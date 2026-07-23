//! Before/after pair capture around an action: the raw material for effect
//! validation. CDP-side signals (DOM mutations, aria changes, network fetches)
//! ride along as the free labels 3.2 trains against — same auto-labeling bet
//! as the detector, applied to time instead of space.
//!
//! Signals are counted page-side by an injected MutationObserver + the
//! Performance API: no CDP event-stream plumbing, and the counts are read in
//! the same instant as the after-screenshot.

use anyhow::Result;
use chromiumoxide::Page;
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchMouseEventParams, DispatchMouseEventType, MouseButton,
};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::ScreenshotParams;

async fn shot(page: &Page) -> Result<Vec<u8>> {
    Ok(page
        .screenshot(
            ScreenshotParams::builder()
                .format(CaptureScreenshotFormat::Png)
                .build(),
        )
        .await?)
}

/// One captured action: what the page looked like around it and what the DOM
/// admits happened. `signals` are ground truth for training, NOT available at
/// verb runtime (canvas!) — the effect model must learn to see them in pixels.
#[derive(Debug)]
pub struct ActionPair {
    pub before_png: Vec<u8>,
    pub after_png: Vec<u8>,
    /// Activity during the action window, ambient already subtracted.
    pub signals: ActionSignals,
    /// What the page did on its own in an equal window BEFORE the action —
    /// animations, timers, analytics. The noise floor the label is judged above.
    pub ambient: ActionSignals,
}

/// Canonical signal type lives in verbivore-dataset so training reads the same
/// shape the harvester writes.
pub use verbivore_dataset::EffectSignals as ActionSignals;

const OBSERVER_JS: &str = r#"
window.__vb = { mutations: 0, aria: 0, requests: 0 };
new MutationObserver(records => {
    window.__vb.mutations += records.length;
    for (const r of records) {
        if (r.type === 'attributes' && r.attributeName && r.attributeName.startsWith('aria-')) {
            window.__vb.aria += 1;
        }
    }
}).observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
// Count request INTENT, not resource-timing entries: data:/blob:/cached
// requests dodge the timeline, but a wrapped call can't hide.
const origFetch = window.fetch.bind(window);
window.fetch = (...args) => { window.__vb.requests += 1; return origFetch(...args); };
const origOpen = XMLHttpRequest.prototype.open;
XMLHttpRequest.prototype.open = function (...args) {
    window.__vb.requests += 1;
    return origOpen.apply(this, args);
};
'armed'
"#;

// A missing __vb means the document was replaced under us — navigation.
const READ_JS: &str = r#"
JSON.stringify(window.__vb
    ? { mutations: window.__vb.mutations, aria: window.__vb.aria, requests: window.__vb.requests }
    : { navigated: true })
"#;

/// Arms the signal counters on an already-loaded page. Call once per page,
/// BEFORE `click_and_capture`.
pub(crate) async fn arm(page: &Page) -> Result<()> {
    page.evaluate(OBSERVER_JS).await?;
    Ok(())
}

/// Captures a pair around an optional click (None = no-action control pair).
/// `settle_ms` is a fixed wait for v1 — replacing it with the effect model IS
/// phase 3's plot.
pub(crate) async fn click_and_capture(
    page: &Page,
    click: Option<(f64, f64)>,
    settle_ms: u64,
) -> Result<ActionPair> {
    // Control window first: same duration, no action. Whatever fires here is
    // the page's own noise (animations, timers), not the click's doing.
    tokio::time::sleep(std::time::Duration::from_millis(settle_ms)).await;
    let ambient = read_signals(page).await?;

    let before_png = shot(page).await?;
    if let Some((x, y)) = click {
        for kind in [
            DispatchMouseEventType::MousePressed,
            DispatchMouseEventType::MouseReleased,
        ] {
            let mut params = DispatchMouseEventParams::new(kind, x, y);
            params.button = Some(MouseButton::Left);
            params.click_count = Some(1);
            page.execute(params).await?;
        }
    }
    tokio::time::sleep(std::time::Duration::from_millis(settle_ms)).await;
    let after_png = shot(page).await?;
    let total = read_signals(page).await?;

    let signals = if total.navigated {
        // Counters died with the old document; navigation IS the signal.
        ActionSignals {
            navigated: true,
            ..Default::default()
        }
    } else {
        ActionSignals {
            dom_mutations: total.dom_mutations.saturating_sub(ambient.dom_mutations),
            aria_mutations: total.aria_mutations.saturating_sub(ambient.aria_mutations),
            network_requests: total.network_requests.saturating_sub(ambient.network_requests),
            navigated: false,
        }
    };
    Ok(ActionPair {
        before_png,
        after_png,
        signals,
        ambient,
    })
}

async fn read_signals(page: &Page) -> Result<ActionSignals> {
    let raw: String = page.evaluate(READ_JS).await?.into_value()?;
    let counts: serde_json::Value = serde_json::from_str(&raw)?;
    let get = |k: &str| counts.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
    Ok(ActionSignals {
        dom_mutations: get("mutations"),
        aria_mutations: get("aria"),
        network_requests: get("requests"),
        navigated: counts
            .get("navigated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}
