//! C.6: the input contract must be realizable at INFERENCE, not just at
//! harvest. The planned "cheaper runtime approximation" dissolved when C.1
//! landed on getEventListeners(depth=-1): the whole listener pass is ONE
//! protocol call, so the harvest collector IS the runtime rasterizer — this
//! test proves it live and bounds its latency.

use std::time::Instant;
use verbivore_grounding::data::{Letterbox, rasterize_prior};
use verbivore_harvester::{Harvester, Variation, affordance};

#[tokio::test]
#[ignore = "needs the corpus running"]
async fn live_page_to_prior_planes_under_budget() -> anyhow::Result<()> {
    let harvester = Harvester::launch().await?;
    let variation = Variation::default();
    let page = harvester
        .open_page("http://localhost:42002/explore/repos", &variation)
        .await?;
    harvester.settle_render(&page).await?;

    let (vw, vh) = variation.viewport;
    let started = Instant::now();
    let evidence = affordance::collect(&page, vw as f64, vh as f64, variation.dpr).await?;
    let collect_ms = started.elapsed().as_millis();

    let started = Instant::now();
    let lb = Letterbox::fit(
        (vw as f64 * variation.dpr) as u32,
        (vh as f64 * variation.dpr) as u32,
    );
    let prior = rasterize_prior(&evidence, &lb);
    let raster_ms = started.elapsed().as_millis();
    page.close().await.ok();
    harvester.close().await?;

    println!(
        "runtime rasterizer: {} evidence in {collect_ms}ms, planes in {raster_ms}ms",
        evidence.len()
    );
    assert!(!evidence.is_empty(), "a live app page must yield evidence");
    assert!(
        prior.iter().any(|&v| v > 0.0),
        "evidence must rasterize to non-zero heat"
    );
    // Runtime budget: the executor runs grounding at authoring/repair time,
    // so even a generous bound proves viability — but it must not be the
    // per-node protocol walk this design replaced.
    assert!(
        collect_ms < 2000,
        "collect took {collect_ms}ms — the one-call listener pass regressed"
    );
    Ok(())
}
