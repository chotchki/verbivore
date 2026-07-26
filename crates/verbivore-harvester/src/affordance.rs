//! C.1 affordance evidence: what the DOM says about WHERE actions could
//! land, collected as vector facts for the fusion input planes. Two passes:
//! one JS sweep for declarative evidence (cursor, native tags, tabindex,
//! scrollables), one CDP pass for REAL listener geometry — the thing a DOM
//! attribute scan is blind to (the 6.1 addEventListener blind spot, closed).
//! Deliberately independent of the a11y tree: a11y is the label source, and
//! the prior must be free to disagree with labels or the model
//! shortcut-copies it.

use anyhow::Result;
use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::dom::GetContentQuadsParams;
use chromiumoxide::cdp::browser_protocol::dom_debugger::GetEventListenersParams;
use chromiumoxide::cdp::js_protocol::runtime::EvaluateParams;
pub use verbivore_dataset::{AffordanceChannel, AffordanceEvidence, AffordanceSource, Bbox};

/// Declarative evidence in one JS pass. Returns [x, y, w, h, channel, source]
/// tuples in CSS px; channel/source are indices into the enums below.
const DECLARATIVE_JS: &str = r#"
(() => {
    const out = [];
    const push = (el, ch, src) => {
        const r = el.getBoundingClientRect();
        if (r.width < 2 || r.height < 2) return;
        out.push([r.x, r.y, r.width, r.height, ch, src]);
    };
    // channel: 0 pointer, 1 keyboard, 2 scroll — mirrors AffordanceChannel.
    // source: 0 cursor, 1 native, 2 tabindex, 3 scrollable — Listener and
    // Ambient come from the CDP pass, not here.
    for (const el of document.querySelectorAll('*')) {
        const s = getComputedStyle(el);
        if (s.cursor === 'pointer') {
            const p = el.parentElement;
            // Roots only: cursor inherits, so credit the outermost carrier.
            if (!p || getComputedStyle(p).cursor !== 'pointer') push(el, 0, 0);
        }
        const tag = el.tagName;
        if (tag === 'A' && el.href) push(el, 0, 1);
        else if (tag === 'BUTTON' || tag === 'SELECT' || tag === 'SUMMARY') push(el, 0, 1);
        else if (tag === 'INPUT' || tag === 'TEXTAREA' || el.isContentEditable === true) {
            push(el, 0, 1);
            push(el, 1, 1);
        }
        if (el.hasAttribute('tabindex') && el.tabIndex >= 0) push(el, 1, 2);
        if ((s.overflowY === 'auto' || s.overflowY === 'scroll'
             || s.overflowX === 'auto' || s.overflowX === 'scroll')
            && (el.scrollHeight > el.clientHeight + 4 || el.scrollWidth > el.clientWidth + 4))
            push(el, 2, 3);
        if (el.getAttribute('draggable') === 'true') push(el, 2, 1);
        if (el.hasAttribute('onclick')) push(el, 0, 1);
    }
    return out;
})()
"#;

fn channel_of_listener(kind: &str) -> Option<AffordanceChannel> {
    match kind {
        "click" | "dblclick" | "mousedown" | "mouseup" | "pointerdown" | "pointerup"
        | "contextmenu" | "touchstart" | "touchend" => Some(AffordanceChannel::Pointer),
        "keydown" | "keyup" | "keypress" | "input" | "beforeinput" | "compositionstart" => {
            Some(AffordanceChannel::Keyboard)
        }
        "wheel" | "mousewheel" | "scroll" | "touchmove" | "drag" | "dragstart" | "dragover"
        | "drop" => Some(AffordanceChannel::Scroll),
        // mousemove/mouseover/focus/blur are too generic to be affordances.
        _ => None,
    }
}

/// Both passes; rects clamped to the viewport and scaled to screenshot px.
pub async fn collect(
    page: &Page,
    viewport_w: f64,
    viewport_h: f64,
    dpr: f64,
) -> Result<Vec<AffordanceEvidence>> {
    let mut evidence = Vec::new();

    let raw: Vec<(f64, f64, f64, f64, u8, u8)> =
        page.evaluate(DECLARATIVE_JS).await?.into_value()?;
    for (x, y, w, h, ch, src) in raw {
        let channel = match ch {
            0 => AffordanceChannel::Pointer,
            1 => AffordanceChannel::Keyboard,
            _ => AffordanceChannel::Scroll,
        };
        let source = match src {
            0 => AffordanceSource::CursorPointer,
            1 => AffordanceSource::NativeTag,
            2 => AffordanceSource::Tabindex,
            _ => AffordanceSource::Scrollable,
        };
        if let Some(bbox) = clamp(x, y, w, h, viewport_w, viewport_h) {
            evidence.push(AffordanceEvidence {
                bbox: scale(bbox, dpr),
                channel,
                source,
            });
        }
    }

    // Listener pass: resolve window and document once each; depth -1 walks
    // the whole subtree in ONE protocol call. Listeners that carry no node
    // (or sit on document/window) are AMBIENT — real evidence, viewport-wide
    // bbox, near-zero specificity once rasterized.
    for root_expr in ["window", "document"] {
        // Raw Runtime.evaluate: the protocol default returnByValue=false
        // hands back an object REFERENCE — page.evaluate_expression
        // serializes by value, which explodes on window and strips the
        // object id off document.
        let params = EvaluateParams::builder()
            .expression(root_expr)
            .build()
            .map_err(anyhow::Error::msg)?;
        let Ok(eval) = page.execute(params).await else {
            continue;
        };
        let Some(object_id) = eval.result.result.object_id.clone() else {
            continue;
        };
        let params = GetEventListenersParams {
            object_id,
            depth: Some(-1),
            pierce: Some(true),
        };
        let Ok(listeners) = page.execute(params).await else {
            continue;
        };
        for l in &listeners.result.listeners {
            let Some(channel) = channel_of_listener(&l.r#type) else {
                continue;
            };
            let bbox = match &l.backend_node_id {
                Some(id) => {
                    let quads = page
                        .execute(
                            GetContentQuadsParams::builder()
                                .backend_node_id(id.clone())
                                .build(),
                        )
                        .await;
                    quads
                        .ok()
                        .and_then(|q| q.result.quads.first().map(|q| q.inner().clone()))
                        .and_then(|q| quad_bbox(&q))
                        .and_then(|(x, y, w, h)| clamp(x, y, w, h, viewport_w, viewport_h))
                }
                None => None,
            };
            match bbox {
                // Element-scoped rects that fill (nearly) the whole viewport
                // are delegation roots — same information as no rect at all.
                Some(b) if b.w * b.h < 0.9 * viewport_w * viewport_h => {
                    evidence.push(AffordanceEvidence {
                        bbox: scale(b, dpr),
                        channel,
                        source: AffordanceSource::Listener,
                    });
                }
                _ => evidence.push(AffordanceEvidence {
                    bbox: scale(
                        Bbox { x: 0.0, y: 0.0, w: viewport_w, h: viewport_h },
                        dpr,
                    ),
                    channel,
                    source: AffordanceSource::Ambient,
                }),
            }
        }
    }

    // Ambient dedupe: one entry per channel is the information content.
    let mut seen_ambient = [false; 3];
    evidence.retain(|e| {
        if e.source != AffordanceSource::Ambient {
            return true;
        }
        let i = e.channel as usize;
        !std::mem::replace(&mut seen_ambient[i], true)
    });
    Ok(evidence)
}

fn quad_bbox(quad: &[f64]) -> Option<(f64, f64, f64, f64)> {
    if quad.len() < 8 {
        return None;
    }
    let xs = [quad[0], quad[2], quad[4], quad[6]];
    let ys = [quad[1], quad[3], quad[5], quad[7]];
    let min_x = xs.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_x = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let min_y = ys.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_y = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    Some((min_x, min_y, max_x - min_x, max_y - min_y))
}

fn clamp(x: f64, y: f64, w: f64, h: f64, vw: f64, vh: f64) -> Option<Bbox> {
    let x0 = x.max(0.0);
    let y0 = y.max(0.0);
    let x1 = (x + w).min(vw);
    let y1 = (y + h).min(vh);
    (x1 - x0 >= 2.0 && y1 - y0 >= 2.0).then_some(Bbox {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

fn scale(b: Bbox, dpr: f64) -> Bbox {
    Bbox { x: b.x * dpr, y: b.y * dpr, w: b.w * dpr, h: b.h * dpr }
}
