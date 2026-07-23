//! Interaction heuristics: find what LOOKS clickable without asking the a11y
//! tree — cursor:pointer roots, focusable tabindex, inline click handlers,
//! bare anchors/buttons. Anything here that a11y did NOT label becomes an
//! ignore-region: the detector must not be taught that an unlabeled-but-
//! interactive element is background. A11y quality is a gradient; this is
//! the instrument that measures where a page sits on it.
//!
//! Known blind spot: listeners attached via addEventListener are invisible
//! to a DOM scan — the heuristic under-counts on framework-heavy pages, so
//! density ratios are an upper bound on labeling quality, never proof of it.

use anyhow::Result;
use chromiumoxide::Page;
use verbivore_dataset::{Bbox, ElementLabel, iou};

/// Viewport-CSS-px rects of interaction-looking elements. `cursor:pointer`
/// counts only at its ROOT (parent not also pointer) — the style inherits,
/// and without the root test one styled <body> would flag the whole page.
const SCAN_JS: &str = r#"
(() => {
    const out = [];
    const vw = window.innerWidth, vh = window.innerHeight;
    for (const el of document.querySelectorAll('body *')) {
        if (out.length >= 400) break;
        const r = el.getBoundingClientRect();
        if (r.width < 8 || r.height < 8) continue;
        if (r.right <= 0 || r.bottom <= 0 || r.left >= vw || r.top >= vh) continue;
        const style = getComputedStyle(el);
        if (style.visibility === 'hidden' || style.display === 'none') continue;
        const pointer = style.cursor === 'pointer'
            && (!el.parentElement || getComputedStyle(el.parentElement).cursor !== 'pointer');
        const focusable = el.tabIndex >= 0
            && (el.hasAttribute('tabindex') || ['A', 'BUTTON', 'INPUT', 'SELECT', 'TEXTAREA'].includes(el.tagName));
        const handler = el.hasAttribute('onclick');
        const anchor = el.tagName === 'A' && el.hasAttribute('href');
        if (pointer || focusable || handler || anchor) {
            out.push([r.left, r.top, r.width, r.height]);
        }
    }
    return out;
})()
"#;

/// A heuristic rect is "covered" by a label when they substantially overlap
/// or the rect's center sits inside the label — either way the element is
/// accounted for and needs no ignore-region.
const COVERED_IOU: f64 = 0.3;

pub struct InteractionScan {
    /// Heuristic rects with NO covering label, scaled to screenshot px —
    /// the ignore-regions for this capture.
    pub uncovered: Vec<Bbox>,
    /// Total interaction-looking rects seen (covered or not).
    pub looks_interactive: usize,
}

impl InteractionScan {
    /// Fraction of the interactive-LOOKING surface the a11y labels account
    /// for; 1.0 on a fully-labeled page. The 6.2 density gate reads this.
    pub fn coverage(&self) -> f64 {
        if self.looks_interactive == 0 {
            return 1.0;
        }
        1.0 - self.uncovered.len() as f64 / self.looks_interactive as f64
    }
}

/// Runs the scan on a live page and subtracts everything `labels` covers.
/// `labels` and the returned rects are both in screenshot px (css * dpr).
pub async fn scan(page: &Page, labels: &[ElementLabel], dpr: f64) -> Result<InteractionScan> {
    let rects: Vec<(f64, f64, f64, f64)> = page.evaluate(SCAN_JS).await?.into_value()?;
    let looks_interactive = rects.len();
    let uncovered = rects
        .into_iter()
        .map(|(x, y, w, h)| Bbox { x: x * dpr, y: y * dpr, w: w * dpr, h: h * dpr })
        .filter(|rect| {
            let center = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            !labels.iter().any(|l| {
                iou(rect, &l.bbox) >= COVERED_IOU
                    || (center.0 >= l.bbox.x
                        && center.0 <= l.bbox.x + l.bbox.w
                        && center.1 >= l.bbox.y
                        && center.1 <= l.bbox.y + l.bbox.h)
            })
        })
        .collect();
    Ok(InteractionScan { uncovered, looks_interactive })
}
