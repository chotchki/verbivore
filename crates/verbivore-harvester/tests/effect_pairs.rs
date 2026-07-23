use verbivore_harvester::Harvester;

/// Buttons at known positions: one mutates the DOM + flips aria-expanded, one
/// only fires a fetch, and (500,500) is dead space.
const FIXTURE: &str = "data:text/html,<html><body style=\"margin:0\">\
    <button aria-expanded=\"false\" style=\"position:absolute;left:100px;top:100px;width:100px;height:40px\" \
      onclick=\"this.setAttribute('aria-expanded','true');const d=document.createElement('div');d.textContent='opened';d.style.cssText='position:absolute;top:200px;left:100px;width:200px;height:100px;background:%23fa0';document.body.appendChild(d)\">menu</button>\
    <button style=\"position:absolute;left:300px;top:100px;width:100px;height:40px\" \
      onclick=\"fetch('data:text/plain,pong')\">ping</button>\
    </body></html>";

#[tokio::test]
async fn real_click_yields_mutations_and_pixel_change() -> anyhow::Result<()> {
    let harvester = Harvester::launch().await?;
    let pair = harvester
        .capture_action_pair(FIXTURE, (150.0, 120.0), 400)
        .await?;
    harvester.close().await?;

    assert!(pair.signals.dom_mutations > 0, "menu click must mutate");
    assert!(pair.signals.aria_mutations >= 1, "aria-expanded flip counted");
    assert_ne!(
        pair.before_png, pair.after_png,
        "orange panel must change pixels"
    );
    Ok(())
}

#[tokio::test]
async fn dead_click_yields_silence() -> anyhow::Result<()> {
    let harvester = Harvester::launch().await?;
    let pair = harvester
        .capture_action_pair(FIXTURE, (500.0, 500.0), 400)
        .await?;
    harvester.close().await?;

    assert_eq!(pair.signals.dom_mutations, 0, "dead space must not mutate");
    assert_eq!(pair.signals.network_requests, 0);
    assert_eq!(
        pair.before_png, pair.after_png,
        "static page, identical pixels"
    );
    Ok(())
}

#[tokio::test]
async fn network_only_click_is_visible_to_signals() -> anyhow::Result<()> {
    let harvester = Harvester::launch().await?;
    let pair = harvester
        .capture_action_pair(FIXTURE, (350.0, 120.0), 400)
        .await?;
    harvester.close().await?;

    assert!(
        pair.signals.network_requests >= 1,
        "fetch must appear in the resource timeline: {:?}",
        pair.signals
    );
    Ok(())
}
