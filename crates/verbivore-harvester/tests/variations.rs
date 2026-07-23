use verbivore_dataset::Dataset;
use verbivore_harvester::{ColorScheme, Harvester, Variation};

/// Pins the invariant zoom augmentation depends on: after CSS zoom, Chrome's
/// geometry APIs report POST-zoom coordinates, so labels align with the
/// screenshot. If a Chrome update breaks this, this test fails before the
/// dataset silently rots.
#[tokio::test]
async fn zoomed_labels_come_back_in_screenshot_space() -> anyhow::Result<()> {
    let fixture = "data:text/html,<html><body style=\"margin:0\">\
        <button aria-label=\"Zoomed\" style=\"position:absolute;left:100px;top:200px;width:120px;height:40px\">Z</button>\
        </body></html>";

    let harvester = Harvester::launch().await?;
    let snap = harvester
        .snapshot_with(
            fixture,
            &Variation {
                zoom: 1.25,
                ..Variation::default()
            },
        )
        .await?;
    harvester.close().await?;

    let label = snap
        .labels
        .iter()
        .find(|l| l.name.as_deref() == Some("Zoomed"))
        .unwrap_or_else(|| panic!("zoomed button missing; got {:?}", snap.labels));
    // 100,200,120,40 css px at zoom 1.25 -> 125,250,150,50 screenshot px.
    assert!(
        (label.bbox.x - 125.0).abs() < 3.0 && (label.bbox.y - 250.0).abs() < 3.0,
        "zoomed origin off: {:?}",
        label.bbox
    );
    assert!(
        (label.bbox.w - 150.0).abs() < 3.0 && (label.bbox.h - 50.0).abs() < 3.0,
        "zoomed size off: {:?}",
        label.bbox
    );
    Ok(())
}

/// At dpr 2 the screenshot has double the pixels and labels must scale with it.
/// The png header is the ground truth the assertion leans on.
#[tokio::test]
async fn hidpi_labels_scale_to_screenshot_pixels() -> anyhow::Result<()> {
    let fixture = "data:text/html,<html><body style=\"margin:0\">\
        <button aria-label=\"Retina\" style=\"position:absolute;left:100px;top:200px;width:120px;height:40px\">R</button>\
        </body></html>";

    let harvester = Harvester::launch().await?;
    let snap = harvester
        .snapshot_with(
            fixture,
            &Variation {
                dpr: 2.0,
                ..Variation::default()
            },
        )
        .await?;
    harvester.close().await?;

    assert_eq!(
        png_dims(&snap.screenshot_png),
        (2560, 1600),
        "png should be viewport * dpr"
    );
    let label = snap
        .labels
        .iter()
        .find(|l| l.name.as_deref() == Some("Retina"))
        .unwrap_or_else(|| panic!("retina button missing; got {:?}", snap.labels));
    // 100,200,120,40 css px at dpr 2 -> 200,400,240,80 screenshot px.
    assert!(
        (label.bbox.x - 200.0).abs() < 4.0
            && (label.bbox.y - 400.0).abs() < 4.0
            && (label.bbox.w - 240.0).abs() < 4.0
            && (label.bbox.h - 80.0).abs() < 4.0,
        "hidpi bbox off: {:?}",
        label.bbox
    );
    Ok(())
}

fn png_dims(png: &[u8]) -> (u32, u32) {
    (
        u32::from_be_bytes(png[16..20].try_into().unwrap()),
        u32::from_be_bytes(png[20..24].try_into().unwrap()),
    )
}

/// Color-scheme emulation must reach the page's media queries, and the sweep
/// must dedupe re-renders that change nothing.
#[tokio::test]
async fn sweep_captures_scheme_variants_and_dedupes_reruns() -> anyhow::Result<()> {
    let fixture = "data:text/html,<html><head><style>\
        @media (prefers-color-scheme: dark) { body { background: %23111; } }\
        </style></head><body>\
        <button aria-label=\"Toggle\">T</button>\
        </body></html>";
    let variations = [
        Variation::default(),
        Variation {
            color_scheme: ColorScheme::Dark,
            ..Variation::default()
        },
    ];

    let dir = tempfile::tempdir()?;
    let ds = Dataset::create(dir.path())?;
    let harvester = Harvester::launch().await?;

    let first = harvester
        .harvest_variations(&ds, fixture, &variations)
        .await?;
    assert_eq!(
        (first.added, first.deduped),
        (2, 0),
        "light and dark should render differently"
    );

    let rerun = harvester
        .harvest_variations(&ds, fixture, &variations)
        .await?;
    assert_eq!(
        (rerun.added, rerun.deduped),
        (0, 2),
        "identical re-renders should all dedupe"
    );
    harvester.close().await?;

    assert_eq!(ds.sample_ids()?.len(), 2);
    Ok(())
}
