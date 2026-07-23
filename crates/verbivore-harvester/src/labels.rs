//! Turns a page's accessibility tree + geometry into training labels: bbox, role,
//! accessible name per interactive element. The DOM does the annotating.

use anyhow::Result;
use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::accessibility::{AxNode, AxValue};
use chromiumoxide::cdp::browser_protocol::dom::GetContentQuadsParams;

/// Viewport-space bounding box. CSS px == screenshot px because the harvester
/// forces device scale factor to 1; break that invariant and every label is
/// silently misaligned with its screenshot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bbox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// One training label: where an interactive element is and what it is.
#[derive(Debug, Clone)]
pub struct ElementLabel {
    pub bbox: Bbox,
    pub role: String,
    pub name: Option<String>,
}

/// A11y roles that count as interactive for detection purposes.
const INTERACTIVE_ROLES: &[&str] = &[
    "button",
    "link",
    "textbox",
    "searchbox",
    "checkbox",
    "radio",
    "combobox",
    "listbox",
    "option",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "tab",
    "switch",
    "slider",
    "spinbutton",
];

pub(crate) async fn extract(
    page: &Page,
    nodes: &[AxNode],
    viewport_w: f64,
    viewport_h: f64,
) -> Result<Vec<ElementLabel>> {
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
        candidates.push(ElementLabel {
            bbox,
            role,
            name: ax_str(node.name.as_ref()),
        });
    }
    occlusion_filter(page, candidates).await
}

/// Drops candidates whose center is covered by something outside their own box.
/// Heuristic: the element under the center must lie inside the candidate's bbox
/// (itself, or a child like an icon). A disjoint or larger hit rect means a
/// modal/drawer/overlay is on top. Known false accept: an occluder small enough
/// to sit entirely inside the candidate's box.
async fn occlusion_filter(
    page: &Page,
    candidates: Vec<ElementLabel>,
) -> Result<Vec<ElementLabel>> {
    if candidates.is_empty() {
        return Ok(candidates);
    }
    let centers: Vec<(f64, f64)> = candidates
        .iter()
        .map(|c| (c.bbox.x + c.bbox.w / 2.0, c.bbox.y + c.bbox.h / 2.0))
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
        .filter_map(|(c, hit)| {
            let (hx, hy, hw, hh) = hit?;
            let inside = hx >= c.bbox.x - TOL
                && hy >= c.bbox.y - TOL
                && hx + hw <= c.bbox.x + c.bbox.w + TOL
                && hy + hh <= c.bbox.y + c.bbox.h + TOL;
            inside.then_some(c)
        })
        .collect())
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
