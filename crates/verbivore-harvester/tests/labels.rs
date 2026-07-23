use verbivore_harvester::Harvester;

/// The fixture plants five interactive elements; only two should survive:
/// offscreen, display:none and overlay-covered must all be filtered.
#[tokio::test]
async fn labels_visible_interactive_elements_only() -> anyhow::Result<()> {
    let fixture = "data:text/html,<html><body style=\"margin:0\">\
        <button aria-label=\"Order pizza\" style=\"position:absolute;left:100px;top:200px;width:120px;height:40px\">Go</button>\
        <a href=\"second\" style=\"position:absolute;left:300px;top:50px\">docs</a>\
        <button style=\"position:absolute;left:-2000px;top:0\">offscreen</button>\
        <button style=\"display:none\">hidden</button>\
        <button aria-label=\"covered\" style=\"position:absolute;left:500px;top:500px;width:100px;height:40px\">covered</button>\
        <div style=\"position:absolute;left:480px;top:480px;width:200px;height:100px;background:%23888\"></div>\
        </body></html>";

    let harvester = Harvester::launch().await?;
    let snap = harvester.snapshot(fixture).await?;
    harvester.close().await?;
    let labels = snap.labels;

    let pizza = labels
        .iter()
        .find(|l| l.name.as_deref() == Some("Order pizza"))
        .unwrap_or_else(|| panic!("pizza button missing; got {labels:?}"));
    assert_eq!(pizza.role, "button");
    assert!(
        (pizza.bbox.x - 100.0).abs() < 2.0 && (pizza.bbox.y - 200.0).abs() < 2.0,
        "bbox origin off: {:?}",
        pizza.bbox
    );
    assert!(
        (pizza.bbox.w - 120.0).abs() < 2.0 && (pizza.bbox.h - 40.0).abs() < 2.0,
        "bbox size off: {:?}",
        pizza.bbox
    );

    assert!(
        labels
            .iter()
            .any(|l| l.role == "link" && l.name.as_deref() == Some("docs")),
        "link missing; got {labels:?}"
    );
    for leaked in ["offscreen", "hidden", "covered"] {
        assert!(
            !labels.iter().any(|l| l.name.as_deref() == Some(leaked)),
            "{leaked} should have been filtered; got {labels:?}"
        );
    }
    Ok(())
}
