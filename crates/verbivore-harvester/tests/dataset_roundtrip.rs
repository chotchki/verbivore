use verbivore_dataset::Dataset;
use verbivore_harvester::{Harvester, VIEWPORT_H, VIEWPORT_W};

/// Harvest a fixture straight into a dataset and read it back — the full
/// capture-to-disk path the corpus sweeps will use.
#[tokio::test]
async fn harvested_snapshot_survives_the_dataset_round_trip() -> anyhow::Result<()> {
    let fixture = "data:text/html,<html><body>\
        <button aria-label=\"Save\">Save</button>\
        <a href=\"second\">read more</a>\
        </body></html>";

    let harvester = Harvester::launch().await?;
    let snap = harvester.snapshot(fixture).await?;
    harvester.close().await?;

    let dir = tempfile::tempdir()?;
    let ds = Dataset::create(dir.path())?;
    let out = ds.add(
        fixture,
        VIEWPORT_W,
        VIEWPORT_H,
        1.0,
        snap.labels.clone(),
        snap.ignore.clone(),
        &snap.screenshot_png,
    )?;
    assert!(!out.deduped);

    let meta = Dataset::open(dir.path())?.meta(&out.id)?;
    assert_eq!(meta.labels, snap.labels);
    assert_eq!(meta.viewport_w, VIEWPORT_W);
    let png = std::fs::read(ds.png_path(&out.id))?;
    assert_eq!(png, snap.screenshot_png);
    Ok(())
}
