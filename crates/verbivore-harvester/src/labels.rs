//! Turns a page's accessibility tree + geometry into training labels: bbox, role,
//! accessible name per interactive element. The DOM does the annotating.

use std::collections::HashMap;

use anyhow::Result;
use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::accessibility::{AxNode, AxValue};
use chromiumoxide::cdp::browser_protocol::dom::GetContentQuadsParams;
pub use verbivore_dataset::{Bbox, ElementLabel, INTERACTIVE_ROLES};

/// Ax roles that scope tasks: an element inside one of these belongs to a
/// nameable region ("login form", "main navigation") — the container intents
/// verbs are scoped by.
pub const CONTAINER_ROLES: &[&str] = &[
    "form",
    "dialog",
    "alertdialog",
    "navigation",
    "search",
    "toolbar",
    "menu",
    "menubar",
    "tablist",
    "banner",
    "complementary",
    "contentinfo",
    "region",
];

/// The nearest container ancestor of an interactive element.
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerInfo {
    pub role: String,
    pub name: Option<String>,
}

/// An interactive element plus where it lives — the crawler's raw material.
#[derive(Debug, Clone)]
pub struct LabeledElement {
    pub label: ElementLabel,
    pub container: Option<ContainerInfo>,
}

/// Candidates and the occlusion hit-test work in CSS px (elementFromPoint's
/// space); scaling by dpr into screenshot space is the LAST step.
pub(crate) async fn extract(
    page: &Page,
    nodes: &[AxNode],
    viewport_w: f64,
    viewport_h: f64,
    dpr: f64,
) -> Result<Vec<ElementLabel>> {
    Ok(extract_full(page, nodes, viewport_w, viewport_h, dpr)
        .await?
        .into_iter()
        .map(|e| e.label)
        .collect())
}

/// Like `extract`, but each element carries its nearest container ancestor —
/// found by walking `parent_id` links until a [`CONTAINER_ROLES`] role.
pub(crate) async fn extract_full(
    page: &Page,
    nodes: &[AxNode],
    viewport_w: f64,
    viewport_h: f64,
    dpr: f64,
) -> Result<Vec<LabeledElement>> {
    let by_id: HashMap<&str, &AxNode> =
        nodes.iter().map(|n| (n.node_id.inner().as_str(), n)).collect();
    let mut candidates = Vec::new();
    for node in nodes {
        if node.ignored {
            continue;
        }
        let Some(role) = ax_str(node.role.as_ref()) else {
            continue;
        };
        if !INTERACTIVE_ROLES.contains(&role.as_str()) {
            continue;
        }
        let Some(backend_id) = node.backend_dom_node_id.clone() else {
            continue;
        };
        // Nodes in the tree but not rendered error out here — that's the filter.
        let Ok(quads) = page
            .execute(
                GetContentQuadsParams::builder()
                    .backend_node_id(backend_id)
                    .build(),
            )
            .await
        else {
            continue;
        };
        let Some(bbox) = quads.result.quads.first().and_then(|q| quad_to_bbox(q.inner()))
        else {
            continue;
        };
        let Some(bbox) = clamp_to_viewport(bbox, viewport_w, viewport_h) else {
            continue;
        };
        candidates.push((
            ElementLabel {
                bbox,
                role,
                name: ax_str(node.name.as_ref()),
            },
            container_of(node, &by_id),
        ));
    }
    let visible = occlusion_filter(page, candidates).await?;
    Ok(visible
        .into_iter()
        .map(|(mut label, container)| {
            label.bbox = Bbox {
                x: label.bbox.x * dpr,
                y: label.bbox.y * dpr,
                w: label.bbox.w * dpr,
                h: label.bbox.h * dpr,
            };
            LabeledElement { label, container }
        })
        .collect())
}

/// Nearest ancestor with a container role. Ignored ancestors still link the
/// chain — only their ROLE is disqualified, not their position in it.
fn container_of(node: &AxNode, by_id: &HashMap<&str, &AxNode>) -> Option<ContainerInfo> {
    let mut current = node.parent_id.as_ref();
    // Depth guard: a cycle in parent links must not hang the harvest.
    for _ in 0..64 {
        let parent = by_id.get(current?.inner().as_str())?;
        if !parent.ignored
            && let Some(role) = ax_str(parent.role.as_ref())
            && CONTAINER_ROLES.contains(&role.as_str())
        {
            return Some(ContainerInfo {
                role,
                name: ax_str(parent.name.as_ref()),
            });
        }
        current = parent.parent_id.as_ref();
    }
    None
}

/// Drops candidates whose center is covered by something outside their own box.
/// Heuristic: the element under the center must lie inside the candidate's bbox
/// (itself, or a child like an icon). A disjoint or larger hit rect means a
/// modal/drawer/overlay is on top. Known false accept: an occluder small enough
/// to sit entirely inside the candidate's box.
async fn occlusion_filter(
    page: &Page,
    candidates: Vec<(ElementLabel, Option<ContainerInfo>)>,
) -> Result<Vec<(ElementLabel, Option<ContainerInfo>)>> {
    if candidates.is_empty() {
        return Ok(candidates);
    }
    let centers: Vec<(f64, f64)> = candidates
        .iter()
        .map(|(c, _)| (c.bbox.x + c.bbox.w / 2.0, c.bbox.y + c.bbox.h / 2.0))
        .collect();
    let expr = format!(
        "{}.map(([x, y]) => {{ const el = document.elementFromPoint(x, y); \
         if (!el) return null; const r = el.getBoundingClientRect(); \
         return [r.x, r.y, r.width, r.height]; }})",
        serde_json::to_string(&centers)?
    );
    let hits: Vec<Option<(f64, f64, f64, f64)>> = page.evaluate(expr).await?.into_value()?;

    const TOL: f64 = 1.5;
    Ok(candidates
        .into_iter()
        .zip(hits)
        .filter_map(|(candidate, hit)| {
            let (hx, hy, hw, hh) = hit?;
            let c = &candidate.0;
            let inside = hx >= c.bbox.x - TOL
                && hy >= c.bbox.y - TOL
                && hx + hw <= c.bbox.x + c.bbox.w + TOL
                && hy + hh <= c.bbox.y + c.bbox.h + TOL;
            inside.then_some(candidate)
        })
        .collect())
}

/// Splits link labels into visually-EVIDENT (kept) and pointer-only
/// (demoted). A link styled identically to its parent text — no color shift,
/// no underline, no weight change, no background — is invisible in a static
/// screenshot at ANY resolution; its only affordance is cursor:pointer,
/// which pixels don't carry. Labeling those teaches the detector noise
/// (measured: 31% of corpus links, 95% on wordpress, link ap 0.012 with the
/// LARGEST links scoring 0.000 — styling, not resolution, is the wall).
/// Harvest-only: the executor still resolves any link through the a11y tree.
pub(crate) async fn demote_invisible_links(
    page: &Page,
    labels: Vec<ElementLabel>,
    dpr: f64,
) -> Result<(Vec<ElementLabel>, usize)> {
    let link_centers: Vec<(f64, f64)> = labels
        .iter()
        .filter(|l| l.role == "link")
        // Labels are screenshot px; elementFromPoint wants CSS px.
        .map(|l| ((l.bbox.x + l.bbox.w / 2.0) / dpr, (l.bbox.y + l.bbox.h / 2.0) / dpr))
        .collect();
    if link_centers.is_empty() {
        return Ok((labels, 0));
    }
    let expr = format!(
        "{}.map(([x, y]) => {{ \
            let el = document.elementFromPoint(x, y); \
            if (!el) return true; \
            el = el.closest('a') || el; \
            const s = getComputedStyle(el); \
            const p = el.parentElement ? getComputedStyle(el.parentElement) : s; \
            return s.color !== p.color \
                || s.textDecorationLine.includes('underline') \
                || (parseInt(s.fontWeight) >= 600 && parseInt(p.fontWeight) < 600) \
                || s.backgroundColor !== p.backgroundColor; \
         }})",
        serde_json::to_string(&link_centers)?
    );
    let evident: Vec<bool> = page.evaluate(expr).await?.into_value()?;
    let mut verdicts = evident.into_iter();
    let mut demoted = 0usize;
    let kept = labels
        .into_iter()
        .filter(|l| {
            if l.role != "link" {
                return true;
            }
            let keep = verdicts.next().unwrap_or(true);
            if !keep {
                demoted += 1;
            }
            keep
        })
        .collect();
    Ok((kept, demoted))
}

fn quad_to_bbox(quad: &[f64]) -> Option<Bbox> {
    if quad.len() < 8 {
        return None;
    }
    let xs = [quad[0], quad[2], quad[4], quad[6]];
    let ys = [quad[1], quad[3], quad[5], quad[7]];
    let min_x = xs.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_x = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let min_y = ys.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_y = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    Some(Bbox {
        x: min_x,
        y: min_y,
        w: max_x - min_x,
        h: max_y - min_y,
    })
}

/// Intersects with the viewport; None when what remains is too small to click.
fn clamp_to_viewport(b: Bbox, vw: f64, vh: f64) -> Option<Bbox> {
    let x0 = b.x.max(0.0);
    let y0 = b.y.max(0.0);
    let x1 = (b.x + b.w).min(vw);
    let y1 = (b.y + b.h).min(vh);
    (x1 - x0 >= 2.0 && y1 - y0 >= 2.0).then_some(Bbox {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

pub(crate) fn ax_str(v: Option<&AxValue>) -> Option<String> {
    v.and_then(|v| v.value.as_ref())
        .and_then(|j| j.as_str().map(str::to_owned))
}
