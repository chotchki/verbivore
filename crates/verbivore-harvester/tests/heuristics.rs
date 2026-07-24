//! The labeler-upgrade contract: interactive-LOOKING elements without a11y
//! labeling become ignore-regions, and a page that is mostly unlabeled
//! clickables fails the density gate instead of teaching blindness.

use verbivore_harvester::{Harvester, MIN_LABEL_COVERAGE, Variation};

/// One real button (a11y-labeled) + one cursor:pointer div-soup "button"
/// (invisible to the a11y tree) + inert text.
const MIXED: &str = "data:text/html,<html><body style=\"margin:0\">\
    <button style=\"position:absolute;left:50px;top:50px;width:120px;height:40px\">Real</button>\
    <div style=\"position:absolute;left:300px;top:50px;width:120px;height:40px;cursor:pointer\" \
      onclick=\"this.textContent='clicked'\">Fake button</div>\
    <p style=\"position:absolute;left:50px;top:200px\">just text</p>\
    </body></html>";

/// NOTHING labeled, everything clickable-looking: the wild-web nightmare page.
const DIV_SOUP: &str = "data:text/html,<html><body style=\"margin:0\">\
    <div style=\"cursor:pointer;width:200px;height:40px\" onclick=\"1\">a</div>\
    <div style=\"cursor:pointer;width:200px;height:40px\" onclick=\"1\">b</div>\
    <div style=\"cursor:pointer;width:200px;height:40px\" onclick=\"1\">c</div>\
    </body></html>";

#[tokio::test]
async fn unlabeled_clickables_become_ignore_regions() -> anyhow::Result<()> {
    let harvester = Harvester::launch().await?;
    let snap = harvester.snapshot(MIXED).await?;
    harvester.close().await?;

    assert!(
        snap.labels.iter().any(|l| l.role == "button"),
        "the real button labels: {:?}",
        snap.labels
    );
    let covers_fake = snap
        .ignore
        .iter()
        .any(|b| b.x >= 280.0 && b.x <= 320.0 && b.y >= 30.0 && b.y <= 70.0);
    assert!(covers_fake, "the div-soup button must be ignored: {:?}", snap.ignore);
    assert!(
        snap.label_coverage < 1.0,
        "coverage must reflect the miss: {}",
        snap.label_coverage
    );
    Ok(())
}

#[tokio::test]
async fn div_soup_fails_the_density_gate() -> anyhow::Result<()> {
    let harvester = Harvester::launch().await?;
    let snap = harvester.snapshot(DIV_SOUP).await?;
    harvester.close().await?;

    assert!(
        snap.label_coverage < MIN_LABEL_COVERAGE,
        "an unlabeled-clickable page must not clear the gate: {}",
        snap.label_coverage
    );
    Ok(())
}

/// One classically-styled link, one styled EXACTLY like its surrounding text
/// (pointer cursor is its only affordance — invisible in a screenshot).
const LINK_CONTRAST: &str = "data:text/html,<html><body style=\"margin:0;color:%23222;font-weight:400\">\
    <p style=\"position:absolute;left:40px;top:40px;width:400px\">Some text with \
      <a href=\"/evident\" style=\"color:%230645ad;text-decoration:underline\">an evident link</a> inside.</p>\
    <p style=\"position:absolute;left:40px;top:120px;width:400px\">More text with \
      <a href=\"/invisible\" style=\"color:%23222;text-decoration:none;cursor:pointer\">a camouflaged link</a> inside.</p>\
    </body></html>";

#[tokio::test]
async fn pointer_only_links_demote_to_ignore() -> anyhow::Result<()> {
    let harvester = Harvester::launch().await?;
    let snap = harvester.snapshot(LINK_CONTRAST).await?;
    harvester.close().await?;

    let links: Vec<_> = snap.labels.iter().filter(|l| l.role == "link").collect();
    assert_eq!(links.len(), 1, "only the evident link stays labeled: {links:?}");
    assert_eq!(links[0].name.as_deref(), Some("an evident link"));
    // The camouflaged link's area lands in ignore via the anchor heuristic.
    let covered = snap
        .ignore
        .iter()
        .any(|b| b.y > 100.0 && b.y < 160.0 && b.x > 40.0);
    assert!(covered, "camouflaged link must be ignored, not background: {:?}", snap.ignore);
    Ok(())
}
