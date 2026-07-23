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
    let mut seen: HashSet<String> = HashSet::from([normalize(start_url)]);

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
            let clean = normalize(&href);
            let lower = clean.to_lowercase();
            if !same_host(&clean, start_url)
                || deny.iter().any(|d| lower.contains(d.as_str()))
                || !seen.insert(clean.clone())
            {
                continue;
            }
            frontier.push_back(clean);
        }
    }
    Ok(report)
}

/// Fragment stripped: #anchors are the same document.
fn normalize(url: &str) -> String {
    url.split('#').next().unwrap_or(url).to_string()
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
}
