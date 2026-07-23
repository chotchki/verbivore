//! Same-host BFS over an app, proposing candidates per page. The frontier is
//! href discovery only — the crawler NAVIGATES, it never clicks. Denied url
//! fragments (logout, delete...) guard app state; the corpus apps are
//! disposable but hygiene is free.

use std::collections::{HashSet, VecDeque};

use anyhow::Result;
use verbivore_harvester::{Harvester, Variation};
use verbivore_verb::VerbStore;

use crate::propose::{ProposalContext, propose};

pub struct CrawlReport {
    pub pages: usize,
    pub proposed: usize,
    /// Ids that already existed in the store — accepted or previously
    /// reviewed records are never clobbered by a re-crawl.
    pub skipped_existing: usize,
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
    ".svg", ".ico", ".webp", ".woff", ".woff2", ".rss", ".atom",
];

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
    let mut report = CrawlReport { pages: 0, proposed: 0, skipped_existing: 0 };
    let mut frontier: VecDeque<String> = VecDeque::from([start_url.to_string()]);
    let mut guard = FrontierGuard::new(start_url, deny, max_pages);

    while let Some(url) = frontier.pop_front() {
        if report.pages >= max_pages {
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

        let elements = harvester.page_map(&page, &variation).await?;
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

        let hrefs: Vec<String> = page
            .evaluate("[...document.querySelectorAll('a[href]')].map(a => a.href)")
            .await?
            .into_value()?;
        page.close().await.ok();
        for href in hrefs {
            if let Some(clean) = guard.admit(&href) {
                frontier.push_back(clean);
            }
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
    let mut frontier: VecDeque<String> = VecDeque::from([start_url.to_string()]);
    let mut guard = FrontierGuard::new(start_url, deny, max_pages);
    while let Some(url) = frontier.pop_front() {
        if visited.len() >= max_pages {
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
        let hrefs: Vec<String> = page
            .evaluate("[...document.querySelectorAll('a[href]')].map(a => a.href)")
            .await?
            .into_value()?;
        page.close().await.ok();
        for href in hrefs {
            if let Some(clean) = guard.admit(&href) {
                frontier.push_back(clean);
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
