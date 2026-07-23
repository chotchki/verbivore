//! Same-host BFS over an app, proposing candidates per page. The frontier is
//! href discovery only — the crawler NAVIGATES, it never clicks. Denied url
//! fragments (logout, delete...) guard app state; the corpus apps are
//! disposable but hygiene is free.

use std::collections::HashSet;

use anyhow::Result;
use verbivore_harvester::{Harvester, LabeledElement, Variation};
use verbivore_verb::VerbStore;

use crate::propose::{ProposalContext, propose};

pub struct CrawlReport {
    pub pages: usize,
    pub proposed: usize,
    /// Ids that already existed in the store — accepted or previously
    /// reviewed records are never clobbered by a re-crawl.
    pub skipped_existing: usize,
    /// True when element novelty dried up before the page budget did.
    pub saturated: bool,
}

/// Element-novelty saturation: the STOPPING rule (selection belongs to the
/// farthest-first frontier; each mechanism does what it's good at). Url
/// distance can't detect sameness — /wiki/A and /wiki/B keep a constant
/// leaf-segment distance forever — but their element POPULATIONS are nearly
/// identical, which is chris's actual signal: are we still capturing new
/// elements? Template key = (role, container role, height bucket): names
/// and widths vary with content, heights are line-height-quantized and
/// characterize the control type.
pub struct NoveltyGauge {
    seen: HashSet<(String, Option<String>, i64)>,
    dry_streak: usize,
}

/// Consecutive zero-novelty pages before the walk calls itself done. The
/// page budget stays as the hard ceiling above this.
const SATURATION_PATIENCE: usize = 3;
const HEIGHT_BUCKET_PX: f64 = 16.0;

impl NoveltyGauge {
    pub fn new() -> Self {
        Self { seen: HashSet::new(), dry_streak: 0 }
    }

    /// Records a page's elements; returns how many were novel templates.
    pub fn observe(&mut self, elements: &[LabeledElement]) -> usize {
        let mut novel = 0;
        for e in elements {
            let key = (
                e.label.role.clone(),
                e.container.as_ref().map(|c| c.role.clone()),
                (e.label.bbox.h / HEIGHT_BUCKET_PX).round() as i64,
            );
            if self.seen.insert(key) {
                novel += 1;
            }
        }
        if novel == 0 {
            self.dry_streak += 1;
        } else {
            self.dry_streak = 0;
        }
        novel
    }

    pub fn saturated(&self) -> bool {
        self.dry_streak >= SATURATION_PATIENCE
    }
}

impl Default for NoveltyGauge {
    fn default() -> Self {
        Self::new()
    }
}

pub const DEFAULT_DENY: &[&str] = &["logout", "signout", "sign_out", "delete", "destroy", "remove"];

/// Hosts must match exactly (scheme-insensitive) for a url to enter the
/// frontier — the crawl stays inside the app it was pointed at.
fn same_host(a: &str, b: &str) -> bool {
    fn host(u: &str) -> &str {
        let rest = u.split_once("://").map(|(_, r)| r).unwrap_or(u);
        rest.split(['/', '?', '#']).next().unwrap_or("")
    }
    !host(a).is_empty() && host(a) == host(b)
}

/// Loop armor for the frontier. Exact-url dedup alone loses to parameterized
/// url families — measured on this corpus: every gitea page links
/// `login?redirect_to=<itself>`, so the family grows by one per page visited
/// and `seen` never fires; mediawiki's `?oldid=` diff space is unbounded by
/// construction. Families (path, query stripped) get a small budget instead:
/// a few parameterized samples still contribute layout variation, the
/// explosion doesn't.
struct FrontierGuard {
    start_url: String,
    deny: Vec<String>,
    seen: HashSet<String>,
    family_counts: std::collections::HashMap<String, usize>,
    enqueued: usize,
    max_enqueued: usize,
}

/// Urls per (host, path) family; the third+ query variant is where the
/// explosion starts, not the variation.
const FAMILY_BUDGET: usize = 3;
/// Path segments beyond this smell like a self-referential path trap.
const MAX_PATH_DEPTH: usize = 8;
/// Assets the harvester can't label anyway — a frontier slot on a .txt is a
/// page of coverage lost.
const SKIP_EXTENSIONS: &[&str] = &[
    ".css", ".js", ".json", ".xml", ".txt", ".pdf", ".zip", ".png", ".jpg", ".jpeg", ".gif",
    ".svg", ".ico", ".webp", ".woff", ".woff2", ".rss", ".atom", ".md",
];
/// Machine endpoints that render no document (measured leaks: mediawiki's
/// api.php atom feed hides its format in a query VALUE, gitea's /api/swagger).
const SKIP_SUBSTRINGS: &[&str] = &["/api.php", "/api/", "feedformat="];

/// Template-shaped url tokens: path segments + query KEYS (values are
/// content identity, keys are template identity — ?oldid=7 and ?oldid=9 are
/// the same page shape).
fn url_tokens(url: &str) -> HashSet<String> {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let (path, query) = match rest.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (rest, None),
    };
    let mut tokens: HashSet<String> = path
        .split('/')
        .skip(1) // the host authority is shared by construction (same_host)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect();
    if let Some(q) = query {
        tokens.extend(
            q.split('&')
                .filter_map(|kv| kv.split('=').next())
                .filter(|k| !k.is_empty())
                .map(|k| format!("?{}", k.to_lowercase())),
        );
    }
    tokens
}

/// 1 - Jaccard similarity; 1.0 = no shared structure. Chris's instinct was
/// hamming distance — right goal, wrong metric: hamming is position-aligned
/// and undefined across lengths, so two articles of the SAME template can
/// score farther apart than an article and the admin panel. Token overlap
/// measures the thing the corpus actually wants: template unlikeness.
fn jaccard_distance(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0; // both roots: identical, not distant
    }
    1.0 - a.intersection(b).count() as f64 / union as f64
}

/// The frontier with its selection policy: guarded admission + FARTHEST-FIRST
/// visiting order (greedy k-center) — each pick maximizes the minimum
/// distance to every page already visited, so a truncating budget gets spent
/// across templates instead of down whichever list the first page linked.
struct Frontier {
    guard: FrontierGuard,
    queue: Vec<(String, HashSet<String>, (f64, f64))>,
    visited: Vec<HashSet<String>>,
    /// Where each visited page's inbound link sat — the LAST selection axis
    /// (position breaks token ties; on canvas/wasm surfaces it is the only
    /// axis with signal, every target sharing one chain and one url).
    visited_positions: Vec<(f64, f64)>,
    /// Path families already visited: a query-only twin of a visited page is
    /// the same template BY CONSTRUCTION, whatever its tokens say — probe-
    /// landed twins carry different chain tokens than their href originals
    /// and would otherwise look novel (measured: ?orgId twins outranked
    /// /admin and burned the saturation streak).
    visited_families: HashSet<String>,
}

impl Frontier {
    fn new(start_url: &str, deny: &[String], max_pages: usize) -> Self {
        Self {
            guard: FrontierGuard::new(start_url, deny, max_pages),
            queue: vec![(start_url.to_string(), url_tokens(start_url), (640.0, 400.0))],
            visited: Vec::new(),
            visited_positions: Vec::new(),
            visited_families: HashSet::new(),
        }
    }

    /// True when the url was admitted (novel, in-scope, under budgets).
    /// `chain_tokens` is the DOM chain that presented this link; the entry's
    /// identity is url tokens UNION chain tokens — one combined distance
    /// check, because which side carries the template signal is per-app
    /// unknowable (opaque routes: the chain knows; plain sidebars: the url
    /// knows; the union lets whichever has information dominate).
    fn push(&mut self, href: &str, chain_tokens: &[String], pos: (f64, f64)) -> bool {
        if let Some(clean) = self.guard.admit(href) {
            let mut tokens = url_tokens(&clean);
            tokens.extend(chain_tokens.iter().cloned());
            self.queue.push((clean, tokens, pos));
            true
        } else {
            false
        }
    }

    fn next(&mut self) -> Option<String> {
        let mut pick = 0usize;
        let mut best = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for (i, (url, tokens, pos)) in self.queue.iter().enumerate() {
            let d = if self.visited_families.contains(&FrontierGuard::family(url)) {
                0.05 // same path as a visited page: same template, tokens lie
            } else {
                self.min_distance(tokens)
            };
            let spat = self
                .visited_positions
                .iter()
                .map(|&v| crate::probe::spatial_distance(*pos, v))
                .fold(f64::INFINITY, f64::min);
            // Token distance decides; position breaks near-ties (LAST axis).
            let better = d > best.0 + 0.051 || (d > best.0 - 0.051 && spat > best.1);
            if better {
                best = (d, spat);
                pick = i;
            }
        }
        if self.queue.is_empty() {
            return None;
        }
        let (url, tokens, pos) = self.queue.swap_remove(pick);
        self.visited_families.insert(FrontierGuard::family(&url));
        self.visited.push(tokens);
        self.visited_positions.push(pos);
        Some(url)
    }

    fn min_distance(&self, tokens: &HashSet<String>) -> f64 {
        self.visited
            .iter()
            .map(|v| jaccard_distance(tokens, v))
            .fold(f64::INFINITY, f64::min)
    }
}

impl FrontierGuard {
    fn new(start_url: &str, deny: &[String], max_pages: usize) -> Self {
        Self {
            start_url: start_url.to_string(),
            deny: deny.to_vec(),
            seen: HashSet::from([normalize(start_url)]),
            family_counts: std::collections::HashMap::new(),
            enqueued: 0,
            // Generous relative to the visit budget; purely a memory bound.
            max_enqueued: max_pages.saturating_mul(20).max(200),
        }
    }

    fn family(url: &str) -> String {
        url.split('?').next().unwrap_or(url).to_string()
    }

    fn admit(&mut self, href: &str) -> Option<String> {
        let clean = normalize(href);
        let lower = clean.to_lowercase();
        if !same_host(&clean, &self.start_url)
            || self.deny.iter().any(|d| lower.contains(d.as_str()))
            || SKIP_EXTENSIONS.iter().any(|e| lower.split('?').next().unwrap_or("").ends_with(e))
            || SKIP_SUBSTRINGS.iter().any(|s| lower.contains(s))
            || self.enqueued >= self.max_enqueued
        {
            return None;
        }
        let path = clean.split_once("://").map(|(_, r)| r).unwrap_or(&clean);
        if path.split('?').next().unwrap_or("").matches('/').count() > MAX_PATH_DEPTH {
            return None;
        }
        let family = Self::family(&clean);
        if *self.family_counts.get(&family).unwrap_or(&0) >= FAMILY_BUDGET {
            return None;
        }
        if !self.seen.insert(clean.clone()) {
            return None;
        }
        *self.family_counts.entry(family).or_insert(0) += 1;
        self.enqueued += 1;
        Some(clean)
    }
}

pub async fn crawl(
    harvester: &Harvester,
    store: &VerbStore,
    app: &str,
    start_url: &str,
    max_pages: usize,
    max_per_page: usize,
    deny: &[String],
) -> Result<CrawlReport> {
    let variation = Variation::default();
    let mut report =
        CrawlReport { pages: 0, proposed: 0, skipped_existing: 0, saturated: false };
    let mut frontier = Frontier::new(start_url, deny, max_pages);
    let mut gauge = NoveltyGauge::new();

    while let Some(url) = frontier.next() {
        if report.pages >= max_pages {
            break;
        }
        if gauge.saturated() {
            report.saturated = true;
            break;
        }
        let page = match harvester.open_page(&url, &variation).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("crawl: skipping {url}: {e:#}");
                continue;
            }
        };
        report.pages += 1;
        harvester.settle_render(&page).await?;

        let elements = harvester.page_map(&page, &variation).await?;
        gauge.observe(&elements);
        let created_at = crate::now_iso();
        let ctx = ProposalContext {
            app,
            url: &url,
            rendering: crate::default_rendering(),
            created_at: &created_at,
            max_per_page,
        };
        for record in propose(&elements, &ctx) {
            if store.load(app, &record.id).is_ok() {
                report.skipped_existing += 1;
            } else {
                store.save(&record)?;
                report.proposed += 1;
            }
        }

        let hrefs = crate::probe::collect_hrefs(&page).await?;
        page.close().await.ok();
        for href in hrefs {
            frontier.push(&href.href, &href.tokens, (href.x, href.y));
        }
    }
    Ok(report)
}

/// Fragment stripped: #anchors are the same document.
fn normalize(url: &str) -> String {
    url.split('#').next().unwrap_or(url).to_string()
}

/// The frontier walk WITHOUT verb proposal: same BFS, same deny list, urls
/// out. This is how harvest url-lists stop being hand-curated — point it at
/// an app, pipe the output into `verbivore harvest`.
pub async fn discover(
    harvester: &Harvester,
    start_url: &str,
    max_pages: usize,
    deny: &[String],
) -> Result<Vec<String>> {
    let variation = Variation::default();
    let mut visited: Vec<String> = Vec::new();
    let mut frontier = Frontier::new(start_url, deny, max_pages);
    let mut gauge = NoveltyGauge::new();
    while let Some(url) = frontier.next() {
        if visited.len() >= max_pages {
            break;
        }
        if gauge.saturated() {
            eprintln!("discover: element novelty saturated after {} pages", visited.len());
            break;
        }
        let page = match harvester.open_page(&url, &variation).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("discover: skipping {url}: {e:#}");
                continue;
            }
        };
        visited.push(url.clone());
        harvester.settle_render(&page).await?;
        let novel = gauge.observe(&harvester.page_map(&page, &variation).await?);
        eprintln!("discover: {url} (+{novel} element templates)");
        let hrefs = crate::probe::collect_hrefs(&page).await?;
        page.close().await.ok();
        let mut admitted = 0usize;
        for href in hrefs {
            if frontier.push(&href.href, &href.tokens, (href.x, href.y)) {
                admitted += 1;
            }
        }
        // Url structure we can't trust (href-dry page): probe navigation
        // targets by DOM-chain diversity instead and admit wherever they land.
        if admitted < 3 {
            for (landed, chain, pos) in
                crate::probe::probe_urls(harvester, &url, &variation, 6).await?
            {
                if frontier.push(&landed, &chain, pos) {
                    eprintln!("discover: probed -> {landed}");
                }
            }
        }
    }
    Ok(visited)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_matching_is_exact() {
        assert!(same_host("http://localhost:42002/a", "http://localhost:42002/b"));
        assert!(!same_host("http://localhost:42002/", "http://localhost:42001/"));
        assert!(!same_host("http://evil.com/localhost:42002", "http://localhost:42002/"));
    }

    fn guard() -> FrontierGuard {
        FrontierGuard::new("http://localhost:42002/", &[], 30)
    }

    #[test]
    fn parameterized_families_hit_their_budget() {
        // The measured gitea trap: login?redirect_to=<every page ever visited>.
        let mut g = guard();
        let mut admitted = 0;
        for i in 0..10 {
            if g.admit(&format!("http://localhost:42002/user/login?redirect_to=%2fpage{i}")).is_some() {
                admitted += 1;
            }
        }
        assert_eq!(admitted, FAMILY_BUDGET, "family must cap, not explode");
        // A different family is unaffected.
        assert!(g.admit("http://localhost:42002/explore/repos").is_some());
    }

    #[test]
    fn depth_extensions_and_dupes_are_rejected() {
        let mut g = guard();
        assert!(g.admit("http://localhost:42002/a/b/c/d/e/f/g/h/i/j").is_none(), "path trap");
        assert!(g.admit("http://localhost:42002/assets/licenses.txt").is_none(), "asset");
        assert!(g.admit("http://localhost:42002/style.css?v=3").is_none(), "asset with query");
        assert!(g.admit("http://localhost:42002/repo").is_some());
        assert!(g.admit("http://localhost:42002/repo#readme").is_none(), "fragment dupe");
    }

    #[test]
    fn tokens_are_template_shaped() {
        let a = url_tokens("http://x/index.php?title=Main&oldid=7");
        let b = url_tokens("http://x/index.php?title=Other&oldid=9");
        assert_eq!(a, b, "query VALUES are content, not template");
        assert!(a.contains("?oldid") && a.contains("index.php"));
    }

    #[test]
    fn farthest_first_spends_the_budget_across_templates() {
        // A wiki-shaped app: the landing page links five articles (same
        // template) plus search and admin. A truncating budget must reach
        // the unlike templates before article #2.
        let mut f = Frontier::new("http://x/", &[], 30);
        assert_eq!(f.next().as_deref(), Some("http://x/"));
        for page in ["a", "b", "c"] {
            f.push(&format!("http://x/wiki/{page}"), &[], (0.0, 0.0));
        }
        f.push("http://x/special/search?q=1", &[], (0.0, 0.0));
        f.push("http://x/admin/settings/users", &[], (0.0, 0.0));

        let picks: Vec<String> = (0..3).filter_map(|_| f.next()).collect();
        let wiki_picks = picks.iter().filter(|u| u.contains("/wiki/")).count();
        assert_eq!(
            wiki_picks, 1,
            "one article, then the unlike templates: {picks:?}"
        );
    }



    #[test]
    fn unified_identity_survives_opaque_urls() {
        // Uuid routes: url tokens are all-different, so url distance alone
        // reads two same-menu links as maximally diverse and the genuinely
        // different template (footer chain) as no more interesting. The
        // chain tokens in the union are what rank the footer link first.
        let menu: Vec<String> =
            ["nav", "ul", "a", "txt:reports"].iter().map(|s| s.to_string()).collect();
        let footer: Vec<String> =
            ["footer", "a", "txt:legal"].iter().map(|s| s.to_string()).collect();
        let mut f = Frontier::new("http://x/", &[], 30);
        assert_eq!(f.next().as_deref(), Some("http://x/"));
        f.push("http://x/view/1b9a2c3d", &menu, (0.0, 0.0));
        f.push("http://x/view/9e8f7a6b", &menu, (0.0, 0.0));
        f.push("http://x/view/5c4d3e2f", &footer, (0.0, 0.0));
        let first = f.next().unwrap();
        let second = f.next().unwrap();
        assert_eq!(first, "http://x/view/1b9a2c3d", "ties fall to enqueue order");
        assert_eq!(
            second, "http://x/view/5c4d3e2f",
            "the footer chain must outrank the menu sibling despite opaque urls"
        );
    }
    #[test]
    fn frontier_has_a_memory_bound() {
        let mut g = FrontierGuard::new("http://x/", &[], 1);
        let mut admitted = 0;
        for i in 0..100_000 {
            if g.admit(&format!("http://x/p{i}")).is_some() {
                admitted += 1;
            }
        }
        assert_eq!(admitted, 200, "enqueue cap must hold");
    }
}

#[cfg(test)]
mod gauge_tests {
    use super::*;
    use verbivore_dataset::Bbox;
    use verbivore_harvester::{ContainerInfo, ElementLabel};

    fn el(role: &str, container: Option<&str>, h: f64) -> LabeledElement {
        LabeledElement {
            label: ElementLabel {
                bbox: Bbox { x: 0.0, y: 0.0, w: 100.0, h },
                role: role.into(),
                name: Some("varies per page".into()),
            },
            container: container.map(|r| ContainerInfo { role: r.into(), name: None }),
        }
    }

    #[test]
    fn template_pages_saturate_and_new_templates_reset() {
        let mut g = NoveltyGauge::new();
        // Article template: nav links + body links.
        let article = vec![el("link", Some("navigation"), 20.0), el("link", None, 20.0)];
        assert_eq!(g.observe(&article), 2, "first article is all novel");
        for _ in 0..SATURATION_PATIENCE {
            assert_eq!(g.observe(&article), 0, "same template, nothing new");
        }
        assert!(g.saturated(), "three dry pages = done");

        // A settings page with form controls resets the streak.
        let mut g = NoveltyGauge::new();
        g.observe(&vec![el("link", Some("navigation"), 20.0)]);
        g.observe(&vec![el("link", Some("navigation"), 20.0)]); // dry 1
        let settings = vec![el("textbox", Some("form"), 36.0), el("button", Some("form"), 36.0)];
        assert_eq!(g.observe(&settings), 2);
        assert!(!g.saturated(), "novelty resets the streak");
    }

    #[test]
    fn names_and_widths_do_not_fake_novelty() {
        // Two wiki articles: same layout, different link names/widths.
        let mut g = NoveltyGauge::new();
        let a = vec![el("link", None, 20.0)];
        let mut b = vec![el("link", None, 20.0)];
        b[0].label.name = Some("totally different name".into());
        b[0].label.bbox.w = 340.0;
        g.observe(&a);
        assert_eq!(g.observe(&b), 0, "content identity is not template novelty");
    }
}
