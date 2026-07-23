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
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::ScreenshotParams;

/// Current-viewport PNG.
pub async fn shot(page: &Page) -> Result<Vec<u8>> {
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
    /// Action-window activity on targets the ambient window never touched.
    /// Suppression is per-(node, mutation-kind) and per request path — NOT
    /// count subtraction, which aliases against periodic tickers whose period
    /// is near the settle window (measured: a 600ms ticker under a 600ms
    /// window mislabeled dead clicks Changed).
    pub signals: ActionSignals,
    /// What the page did on its own BEFORE the action (window of
    /// `max(settle_ms, AMBIENT_MIN_MS)` — longer than the action window is
    /// fine, the suppression list just gets more complete). The noise floor
    /// the label is judged above.
    pub ambient: ActionSignals,
}

/// Canonical signal type lives in verbivore-dataset so training reads the same
/// shape the harvester writes.
pub use verbivore_dataset::EffectSignals as ActionSignals;

// Two-phase observer. Ambient phase: count activity AND remember its targets —
// (node, kind) pairs for mutations, origin+pathname for requests. Action
// phase: activity on remembered targets is suppressed; only NOVEL targets
// count. A click's effect touches nodes the ticker never does. Framework
// re-renders that rebuild children land as childList on their (remembered)
// stable parents, so they suppress too. Known hole: noise with a period
// LONGER than the window never registers as ambient — that's a window-length
// problem no bookkeeping fixes, and why the runtime gate is signals OR visual.
// Re-armable: state resets on every evaluation (each arm gets a fresh ambient
// phase — the executor re-arms per step), but observers and wrappers install
// once per document. Everything reads `window.__vb` at event time, so a reset
// re-points them at the fresh state.
const OBSERVER_JS: &str = r#"
(() => {
    window.__vb = {
        phase: 'ambient',
        ambient: { mutations: 0, aria: 0, requests: 0 },
        action: { mutations: 0, aria: 0, requests: 0 },
        seen: new WeakMap(),
        seenPaths: new Set(),
    };
    if (window.__vb_wired) return;
    window.__vb_wired = true;
    const kindKey = (r) =>
        r.type === 'attributes' ? r.attributeName : r.type === 'characterData' ? '#text' : '#children';
    new MutationObserver(records => {
        const vb = window.__vb;
        for (const r of records) {
            const key = kindKey(r);
            const aria = r.type === 'attributes' && key && key.startsWith('aria-');
            if (vb.phase === 'ambient') {
                let kinds = vb.seen.get(r.target);
                if (!kinds) { kinds = new Set(); vb.seen.set(r.target, kinds); }
                kinds.add(key);
                vb.ambient.mutations += 1;
                if (aria) vb.ambient.aria += 1;
            } else {
                const kinds = vb.seen.get(r.target);
                if (kinds && kinds.has(key)) continue;
                vb.action.mutations += 1;
                if (aria) vb.action.aria += 1;
            }
        }
    }).observe(document, { subtree: true, childList: true, attributes: true, characterData: true });
    // Count request INTENT, not resource-timing entries: data:/blob:/cached
    // requests dodge the timeline, but a wrapped call can't hide. Suppression
    // key drops the query so pollers' cache-busters don't defeat it.
    const reqPath = (u) => {
        try { const p = new URL(u instanceof Request ? u.url : String(u), location.href); return p.origin + p.pathname; }
        catch (e) { return String(u); }
    };
    const note = (u) => {
        const vb = window.__vb, path = reqPath(u);
        if (vb.phase === 'ambient') { vb.seenPaths.add(path); vb.ambient.requests += 1; }
        else if (!vb.seenPaths.has(path)) { vb.action.requests += 1; }
    };
    const origFetch = window.fetch.bind(window);
    window.fetch = (...args) => { note(args[0]); return origFetch(...args); };
    const origOpen = XMLHttpRequest.prototype.open;
    XMLHttpRequest.prototype.open = function (...args) {
        note(args[1]);
        return origOpen.apply(this, args);
    };
})();
'armed'
"#;

// Reads the ambient tallies and flips to the action phase in one evaluate —
// no gap for a mutation to land uncounted. A missing __vb means the document
// was replaced under us — navigation.
const BEGIN_ACTION_JS: &str = r#"
JSON.stringify((() => {
    const vb = window.__vb;
    if (!vb) return { navigated: true };
    vb.phase = 'action';
    return { mutations: vb.ambient.mutations, aria: vb.ambient.aria, requests: vb.ambient.requests };
})())
"#;

const READ_JS: &str = r#"
JSON.stringify(window.__vb
    ? { mutations: window.__vb.action.mutations, aria: window.__vb.action.aria, requests: window.__vb.action.requests }
    : { navigated: true })
"#;

/// The ambient window observes at least this long regardless of `settle_ms`.
/// Public so callers composing the phases directly (the executor) use the
/// same floor the harvester does.
pub const AMBIENT_MIN_MS: u64 = 1500;

/// Arms the signal counters on an already-loaded page: observation enters the
/// AMBIENT phase. Re-arm after any navigation — the observer dies with its
/// document.
pub async fn arm(page: &Page) -> Result<()> {
    page.evaluate(OBSERVER_JS).await?;
    Ok(())
}

/// Ends the ambient phase and returns its tallies; subsequent activity counts
/// as the action's — call this immediately before acting.
pub async fn begin_action(page: &Page) -> Result<ActionSignals> {
    read_signals(page, BEGIN_ACTION_JS).await
}

/// Reads the action-phase tallies (ambient-suppressed page-side). A dead
/// `__vb` reads as `navigated: true`.
pub async fn read_action(page: &Page) -> Result<ActionSignals> {
    read_signals(page, READ_JS).await
}

/// Captures a pair around an optional click (None = no-action control pair).
/// `settle_ms` is a fixed wait for v1 — replacing it with the effect model IS
/// phase 3's plot. The ambient window runs `max(settle_ms, AMBIENT_MIN_MS)`:
/// suppression lists only get MORE complete with longer observation (unlike
/// count subtraction, which needed equal windows), and a periodic source is
/// guaranteed to register once the window covers a full period — 600ms
/// windows were missing 750-900ms tickers, whose next tick then read as the
/// click's doing.
pub(crate) async fn click_and_capture(
    page: &Page,
    click: Option<(f64, f64)>,
    settle_ms: u64,
) -> Result<ActionPair> {
    // Control window first, no action. Whatever fires here is the page's own
    // noise (animations, timers), not the click's doing — its targets become
    // the suppression list for the action window.
    tokio::time::sleep(std::time::Duration::from_millis(settle_ms.max(AMBIENT_MIN_MS))).await;
    let ambient = begin_action(page).await?;

    let before_png = shot(page).await?;
    if let Some((x, y)) = click {
        crate::input::click_at(page, x, y).await?;
    }
    tokio::time::sleep(std::time::Duration::from_millis(settle_ms)).await;
    let after_png = shot(page).await?;
    let action = read_action(page).await?;

    let signals = if action.navigated {
        // Counters died with the old document; navigation IS the signal.
        ActionSignals {
            navigated: true,
            ..Default::default()
        }
    } else {
        action // already ambient-suppressed page-side
    };
    Ok(ActionPair {
        before_png,
        after_png,
        signals,
        ambient,
    })
}

async fn read_signals(page: &Page, js: &str) -> Result<ActionSignals> {
    let raw: String = page.evaluate(js).await?.into_value()?;
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
