//! Verb generation, both entrances: crawl an app wide (every page proposes
//! task-level candidates) or ground one intent phrase into a single record.
//! Both are a11y-driven — the vision detector's debut is canvas content,
//! where there is no tree to rank over.

pub mod crawl;
pub mod generate;
pub mod propose;

use verbivore_verb::RenderingContext;

/// The rendering generation authors evidence under — one canonical context
/// for v1; more variants come from re-grounding runs, not from the crawler.
pub fn default_rendering() -> RenderingContext {
    RenderingContext {
        viewport_w: verbivore_harvester::VIEWPORT_W,
        viewport_h: verbivore_harvester::VIEWPORT_H,
        dpr: 1.0,
        zoom: 1.0,
        color_scheme: "light".into(),
    }
}

/// App label from a url: host + port, slug-safe ("localhost-42002").
pub fn app_label(url: &str) -> String {
    let stripped = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url);
    let host_port = stripped.split(['/', '?', '#']).next().unwrap_or("app");
    propose::slug(host_port)
}

/// RFC3339 now — generation stamps real authoring time.
pub fn now_iso() -> String {
    humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_labels_are_slugs() {
        assert_eq!(app_label("http://localhost:42002/user/login"), "localhost-42002");
        assert_eq!(app_label("file:///tmp/fixtures/noisy.html?v=1"), "unnamed");
        assert_eq!(app_label("https://recon-gen.example.io/x"), "recon-gen-example-io");
    }
}
