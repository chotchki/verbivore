//! Click-probing for pages whose url structure can't be trusted: when href
//! discovery yields nearly nothing, navigation targets are keyed by their
//! DOM CHAIN from the root (chris's design) and probed farthest-first with
//! the same max-min selection the url frontier uses. Chain tokens carry
//! structure (tags, roles, data-testids) AND text-node fragments — text is
//! what separates same-menu siblings, whose structural chains are identical
//! modulo child index (grafana's mega-menu: without text, "Dashboards" and
//! "Explore" are distance-zero twins and one of them never gets probed).

use std::collections::HashSet;

use anyhow::Result;
use serde::Deserialize;
use verbivore_harvester::{Harvester, Variation, input};

/// One clickable navigation candidate as the page presented it.
#[derive(Debug, Clone, Deserialize)]
pub struct NavCandidate {
    pub tokens: Vec<String>,
    /// nth-child css path for re-resolution on a fresh load.
    pub selector: String,
    pub x: f64,
    pub y: f64,
}

/// The shared chain walker: element -> {tokens, selector}. Class names are
/// deliberately ABSENT from tokens: css-in-js hashes churn per build and
/// would make every chain unique. data-testid, roles, tags and text stay —
/// text separates same-menu siblings whose structure is identical.
const CHAIN_FN: &str = r#"
    const direct = (el) => {
        let t = '';
        for (const n of el.childNodes) if (n.nodeType === 3) t += n.textContent;
        return t.trim().toLowerCase().slice(0, 16);
    };
    // Only INFORMATIVE tokens enter the identity: generic tags (div, ul, a)
    // are shared by every link in the app and would drown a distinctive url
    // token under shared chain mass (measured: /admin scored "near" every
    // mega-menu twin and saturation quit before reaching it). Generic tags
    // still anchor the SELECTOR — identity and addressing are different jobs.
    const semantic = new Set(['nav', 'aside', 'header', 'footer', 'main', 'form',
        'dialog', 'table', 'menu', 'section', 'article']);
    const chain = (el) => {
        const tokens = new Set();
        const parts = [];
        let node = el;
        while (node && node.tagName && node.tagName !== 'HTML') {
            const tag = node.tagName.toLowerCase();
            if (semantic.has(tag)) tokens.add(tag);
            const role = node.getAttribute('role');
            if (role) tokens.add('role:' + role);
            const tid = node.getAttribute('data-testid');
            if (tid) tokens.add('tid:' + tid.toLowerCase().slice(0, 24));
            const t = direct(node);
            if (t) tokens.add('txt:' + t);
            let idx = 1, sib = node;
            while ((sib = sib.previousElementSibling)) idx++;
            parts.unshift(tag + ':nth-child(' + idx + ')');
            node = node.parentElement;
        }
        const leaf = (el.textContent || '').trim().toLowerCase().slice(0, 24);
        if (leaf) tokens.add('leaf:' + leaf);
        return { tokens: [...tokens], selector: parts.join('>'), leaf };
    };
"#;

fn collect_js() -> String {
    format!(
        r#"(() => {{
    {CHAIN_FN}
    const out = [];
    const navish = document.querySelectorAll(
        "a[href], [role=link], [role=tab], [role=menuitem], nav button, aside button, header button, [role=navigation] button");
    for (const el of navish) {{
        if (out.length >= 60) break;
        const r = el.getBoundingClientRect();
        if (r.width < 4 || r.height < 4) continue;
        if (r.left >= innerWidth || r.top >= innerHeight || r.right <= 0 || r.bottom <= 0) continue;
        const c = chain(el);
        if (/logout|sign out|signout|delete|remove/.test(c.leaf)) continue;
        out.push({{ tokens: c.tokens, selector: c.selector, x: r.left + r.width / 2, y: r.top + r.height / 2 }});
    }}
    return out;
}})()"#
    )
}

/// Every same-document anchor with the chain that presents it — the frontier
/// unifies these chain tokens with url tokens into ONE identity, because
/// there is no way to know per-app which side carries the template signal
/// (opaque routes: chains carry it; server-rendered sidebars: urls do).
fn hrefs_with_chains_js() -> String {
    format!(
        r#"(() => {{
    {CHAIN_FN}
    const out = [];
    for (const el of document.querySelectorAll('a[href]')) {{
        if (out.length >= 300) break;
        const r = el.getBoundingClientRect();
        out.push({{ href: el.href, tokens: chain(el).tokens, x: r.left + r.width / 2, y: r.top + r.height / 2 }});
    }}
    return out;
}})()"#
    )
}

/// An anchor, the DOM chain it hangs from, and where it sits on the page.
#[derive(Debug, Clone, Deserialize)]
pub struct ChainedHref {
    pub href: String,
    pub tokens: Vec<String>,
    pub x: f64,
    pub y: f64,
}

pub async fn collect_hrefs(page: &chromiumoxide::Page) -> Result<Vec<ChainedHref>> {
    Ok(page.evaluate(hrefs_with_chains_js()).await?.into_value()?)
}

pub async fn collect_nav_candidates(page: &chromiumoxide::Page) -> Result<Vec<NavCandidate>> {
    Ok(page.evaluate(collect_js()).await?.into_value()?)
}

/// Token distances within this margin count as TIES, decided spatially —
/// position is the LAST axis (chris's precedence): pure noise when tokens
/// discriminate, the only signal left when they can't (canvas/wasm targets
/// share one chain and one url; spatial spread is all the diversity there is).
const TOKEN_TIE_MARGIN: f64 = 0.051;
/// Normalizes pixel distance to ~[0,1] against the default viewport diagonal.
const VIEWPORT_DIAG: f64 = 1509.0;

pub(crate) fn spatial_distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    (((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt() / VIEWPORT_DIAG).min(1.0)
}

/// Farthest-first probe order over chain-token sets (greedy k-center, same
/// shape as the url frontier's selection), position breaking token ties.
/// Pure so the policy is testable without a browser.
pub fn probe_order(candidates: &[NavCandidate]) -> Vec<usize> {
    let sets: Vec<HashSet<String>> =
        candidates.iter().map(|c| c.tokens.iter().cloned().collect()).collect();
    let dist = |a: &HashSet<String>, b: &HashSet<String>| {
        let union = a.union(b).count();
        if union == 0 {
            return 0.0;
        }
        1.0 - a.intersection(b).count() as f64 / union as f64
    };
    let mut order: Vec<usize> = Vec::new();
    let mut remaining: Vec<usize> = (0..candidates.len()).collect();
    while !remaining.is_empty() {
        let score = |i: usize| {
            let tok = order
                .iter()
                .map(|&k| dist(&sets[i], &sets[k]))
                .fold(f64::INFINITY, f64::min);
            let spat = order
                .iter()
                .map(|&k| {
                    spatial_distance(
                        (candidates[i].x, candidates[i].y),
                        (candidates[k].x, candidates[k].y),
                    )
                })
                .fold(f64::INFINITY, f64::min);
            (tok, spat)
        };
        let mut pick = 0usize;
        let mut best = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (pos, &i) in remaining.iter().enumerate() {
            let (tok, spat) = score(i);
            let better = tok > best.0 + TOKEN_TIE_MARGIN
                || (tok > best.0 - TOKEN_TIE_MARGIN && spat > best.1);
            if better {
                best = (tok, spat);
                pick = pos;
            }
        }
        order.push(remaining.remove(pick));
    }
    order
}

/// Clicks up to `budget` chain-diverse candidates on fresh loads of
/// `origin`, returning (landed url, the chain that led there) — the chain
/// tokens join the landed url's frontier identity. Every probe gets a
/// clean page: SPA state must not leak between probes.
pub async fn probe_urls(
    harvester: &Harvester,
    origin: &str,
    variation: &Variation,
    budget: usize,
) -> Result<Vec<(String, Vec<String>, (f64, f64))>> {
    let page = harvester.open_page(origin, variation).await?;
    harvester.settle_render(&page).await?;
    let candidates = collect_nav_candidates(&page).await?;
    page.close().await.ok();

    let mut landed = Vec::new();
    for &i in probe_order(&candidates).iter().take(budget) {
        let candidate = &candidates[i];
        let page = match harvester.open_page(origin, variation).await {
            Ok(p) => p,
            Err(_) => continue,
        };
        harvester.settle_render(&page).await?;
        // Re-resolve on the fresh load — layout may have shifted.
        let rect: Option<(f64, f64, f64, f64)> = page
            .evaluate(format!(
                "(() => {{ const el = document.querySelector({sel:?}); if (!el) return null; \
                 const r = el.getBoundingClientRect(); return [r.left, r.top, r.width, r.height]; }})()",
                sel = candidate.selector
            ))
            .await?
            .into_value()
            .unwrap_or(None);
        if let Some((x, y, w, h)) = rect {
            input::click_at(&page, x + w / 2.0, y + h / 2.0).await.ok();
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            if let Ok(url) = page.evaluate("location.href").await
                && let Ok(url) = url.into_value::<String>()
                && url != origin
            {
                landed.push((url, candidate.tokens.clone(), (candidate.x, candidate.y)));
            }
        }
        page.close().await.ok();
    }
    Ok(landed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(tokens: &[&str]) -> NavCandidate {
        NavCandidate {
            tokens: tokens.iter().map(|s| s.to_string()).collect(),
            selector: "nav".into(),
            x: 0.0,
            y: 0.0,
        }
    }


    #[test]
    fn canvas_targets_spread_spatially_when_tokens_cannot_discriminate() {
        // The canvas/wasm case: every click target shares one chain (the
        // canvas element) and one url — token distance is zero across the
        // board, position is the only diversity axis left.
        let canvas = |x: f64, y: f64| NavCandidate {
            tokens: vec!["canvas-chain".into()],
            selector: "canvas".into(),
            x,
            y,
        };
        let cands = vec![
            canvas(100.0, 100.0),
            canvas(110.0, 105.0), // near-duplicate of the first
            canvas(1200.0, 700.0),
            canvas(640.0, 400.0),
        ];
        let order = probe_order(&cands);
        assert_eq!(order[0], 0, "seed");
        assert_eq!(order[1], 2, "opposite corner probes second");
        assert_eq!(order[2], 3, "center third");
        assert_eq!(order[3], 1, "the near-duplicate goes last");
    }
    #[test]
    fn text_tokens_separate_same_menu_siblings() {
        // Identical structural chains; only the text distinguishes them —
        // exactly the mega-menu case the text tokens exist for.
        let menu = ["nav", "ul", "li", "a", "role:menuitem"];
        let a = cand(&[&menu[..], &["leaf:dashboards"]].concat());
        let b = cand(&[&menu[..], &["leaf:explore"]].concat());
        let toolbar = cand(&["header", "button", "role:button", "leaf:settings"]);
        let order = probe_order(&[a, b, toolbar]);
        assert_eq!(order.len(), 3, "text keeps siblings distinct, all get probed");
        assert_eq!(order[1], 2, "the structurally-distant toolbar probes before sibling #2");
    }

    #[test]
    fn probe_order_is_farthest_first_and_total() {
        let cands = vec![
            cand(&["nav", "a", "leaf:one"]),
            cand(&["nav", "a", "leaf:two"]),
            cand(&["footer", "a", "leaf:legal"]),
        ];
        let order = probe_order(&cands);
        assert_eq!(order[0], 0, "first candidate seeds");
        assert_eq!(order[1], 2, "footer is farther than the nav sibling");
        assert_eq!(order[2], 1);
    }
}
