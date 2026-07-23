//! Tests against the live corpus (corpus/docker-compose.yml). Ignored by default
//! so plain `cargo test` never needs docker; run with `cargo test -- --ignored`.

use verbivore_harvester::Harvester;

#[tokio::test]
#[ignore = "needs the corpus running: cd corpus && docker compose up -d"]
async fn snapshots_the_grafana_demo_dashboard() -> anyhow::Result<()> {
    let harvester = Harvester::launch().await?;
    let snap = harvester
        .snapshot("http://localhost:42001/d/verbivore-demo/verbivore-demo")
        .await?;
    harvester.close().await?;

    assert!(snap.screenshot_png.starts_with(&[0x89, b'P', b'N', b'G']));
    assert!(
        snap.html.contains("Grafana"),
        "does not look like a grafana page"
    );
    // Loose on purpose: panels render async and settle detection is Phase 3's
    // job — this only proves the corpus is harvestable at all.
    assert!(
        !snap.ax_nodes.is_empty(),
        "no accessibility nodes from a live dashboard"
    );
    assert!(
        !snap.labels.is_empty(),
        "no interactive labels from a live dashboard"
    );
    Ok(())
}
