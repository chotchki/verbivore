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
        .capture_action_pair(FIXTURE, Some((150.0, 120.0)), 400)
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
        .capture_action_pair(FIXTURE, Some((500.0, 500.0)), 400)
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

/// A ticker whose period EQUALS the settle window: count subtraction aliases
/// (ambient window catches 1 tick, action window 2) and mislabels dead clicks.
/// Per-node suppression must stay silent — same node, same mutation kind.
/// The button touches a node the ticker never does, so it must still count.
const TICKER_FIXTURE: &str = "data:text/html,<html><body style=\"margin:0\">\
    <div id=\"tick\">0</div>\
    <button style=\"position:absolute;left:100px;top:100px;width:100px;height:40px\" \
      onclick=\"document.body.setAttribute('data-hit','1')\">hit</button>\
    <script>let n=0;setInterval(()=>{document.getElementById('tick').textContent=String(++n)},400)</script>\
    </body></html>";

#[tokio::test]
async fn periodic_ticker_does_not_alias_into_dead_clicks() -> anyhow::Result<()> {
    let harvester = Harvester::launch().await?;
    let dead = harvester
        .capture_action_pair(TICKER_FIXTURE, Some((500.0, 500.0)), 400)
        .await?;
    let real = harvester
        .capture_action_pair(TICKER_FIXTURE, Some((150.0, 120.0)), 400)
        .await?;
    harvester.close().await?;

    assert!(
        dead.ambient.dom_mutations > 0,
        "ticker must be visible as ambient noise: {:?}",
        dead.ambient
    );
    assert_eq!(
        dead.signals.dom_mutations, 0,
        "aliased ticker must not leak into a dead click: {:?}",
        dead.signals
    );
    assert!(
        real.signals.dom_mutations > 0,
        "novel-node mutation must survive suppression: {:?}",
        real.signals
    );
    Ok(())
}

#[tokio::test]
async fn network_only_click_is_visible_to_signals() -> anyhow::Result<()> {
    let harvester = Harvester::launch().await?;
    let pair = harvester
        .capture_action_pair(FIXTURE, Some((350.0, 120.0)), 400)
        .await?;
    harvester.close().await?;

    assert!(
        pair.signals.network_requests >= 1,
        "fetch must appear in the resource timeline: {:?}",
        pair.signals
    );
    Ok(())
}

#[tokio::test]
async fn harvest_pairs_produces_labeled_mix() -> anyhow::Result<()> {
    use verbivore_dataset::{EffectLabel, PairDataset};

    let dir = tempfile::tempdir()?;
    let pairs = PairDataset::create(dir.path())?;
    let harvester = Harvester::launch().await?;
    let outcome = harvester.harvest_pairs(&pairs, FIXTURE, 2, 2, 300).await?;
    harvester.close().await?;

    // 2 element clicks + 2 dead clicks + 1 no-action control.
    assert_eq!(outcome.added, 5, "expected 5 fresh pairs");

    let mut changed = 0;
    let mut unchanged = 0;
    let mut controls = 0;
    for id in pairs.pair_ids()? {
        let meta = pairs.meta(&id)?;
        match meta.label {
            EffectLabel::Changed => changed += 1,
            EffectLabel::NoChange => unchanged += 1,
        }
        if meta.click.is_none() {
            controls += 1;
        }
    }
    assert!(changed >= 2, "both real buttons must label Changed");
    assert!(unchanged >= 3, "dead clicks + control must label NoChange");
    assert_eq!(controls, 1, "exactly one no-action control");
    Ok(())
}

#[tokio::test]
async fn navigating_click_labels_changed_via_navigation_signal() -> anyhow::Result<()> {
    let fixture = "data:text/html,<html><body style=\"margin:0\">\
        <a href=\"about:blank\" style=\"position:absolute;left:100px;top:100px;width:100px;height:40px;display:block\">leave</a>\
        </body></html>";
    let harvester = Harvester::launch().await?;
    let pair = harvester
        .capture_action_pair(fixture, Some((150.0, 120.0)), 400)
        .await?;
    harvester.close().await?;

    assert!(pair.signals.navigated, "link click must read as navigation: {:?}", pair.signals);
    Ok(())
}
