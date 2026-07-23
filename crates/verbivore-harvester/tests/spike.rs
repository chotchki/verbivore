use verbivore_harvester::Harvester;

#[tokio::test]
async fn captures_screenshot_dom_and_a11y_from_fixture() -> anyhow::Result<()> {
    let fixture = "data:text/html,<html><head><title>fixture</title></head><body>\
        <h1>Verbivore fixture</h1>\
        <button aria-label=\"Submit order\">Go</button>\
        </body></html>";

    let harvester = Harvester::launch().await?;
    let snap = harvester.snapshot(fixture).await?;
    harvester.close().await?;

    assert!(
        snap.screenshot_png.starts_with(&[0x89, b'P', b'N', b'G']),
        "screenshot is not a png"
    );
    assert!(
        snap.screenshot_png.len() > 1_000,
        "png suspiciously small: {} bytes",
        snap.screenshot_png.len()
    );
    assert!(
        snap.html.contains("Submit order"),
        "aria label missing from captured html"
    );
    assert!(
        snap.ax_nodes
            .iter()
            .any(|n| n.role.as_deref() == Some("button")
                && n.name.as_deref() == Some("Submit order")),
        "button with accessible name not in a11y tree; got {:?}",
        snap.ax_nodes
    );
    Ok(())
}
