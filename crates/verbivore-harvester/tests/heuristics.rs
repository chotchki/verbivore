//! The labeler-upgrade contract: interactive-LOOKING elements without a11y
//! labeling become ignore-regions, and a page that is mostly unlabeled
//! clickables fails the density gate instead of teaching blindness.

use verbivore_harvester::{Harvester, MIN_LABEL_COVERAGE};

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
    // The camouflaged link's area lands in ignore (demoted bbox, appended
    // directly — no longer dependent on the anchor heuristic re-finding it).
    let covered = snap
        .ignore
        .iter()
        .any(|b| b.y > 100.0 && b.y < 160.0 && b.x > 40.0);
    assert!(covered, "camouflaged link must be ignored, not background: {:?}", snap.ignore);
    // 8.4: demoted area is known-and-masked, NOT missing a11y — both anchors
    // are accounted for, so coverage must not read the demotion as a miss.
    assert!(
        snap.label_coverage > 0.99,
        "demotion must not count against coverage: {}",
        snap.label_coverage
    );
    Ok(())
}

/// WordPress in miniature: one real button drowning in camouflaged links.
/// Pre-8.4 the demoted links read as MISSING coverage (1 covered / 7 seen =
/// 0.14) and the density gate rejected the page — punishing the harvest for
/// labeling honestly (wordpress fell 125 -> 52 samples, below the fold floor).
const LINK_FARM: &str = "data:text/html,<html><body style=\"margin:0;color:%23222;font-weight:400\">\
    <button style=\"position:absolute;left:40px;top:20px;width:120px;height:40px\">Real</button>\
    <p style=\"position:absolute;left:40px;top:100px\"><a href=\"/a\" style=\"color:%23222;text-decoration:none\">alpha</a></p>\
    <p style=\"position:absolute;left:40px;top:140px\"><a href=\"/b\" style=\"color:%23222;text-decoration:none\">bravo</a></p>\
    <p style=\"position:absolute;left:40px;top:180px\"><a href=\"/c\" style=\"color:%23222;text-decoration:none\">charlie</a></p>\
    <p style=\"position:absolute;left:40px;top:220px\"><a href=\"/d\" style=\"color:%23222;text-decoration:none\">delta</a></p>\
    <p style=\"position:absolute;left:40px;top:260px\"><a href=\"/e\" style=\"color:%23222;text-decoration:none\">echo</a></p>\
    <p style=\"position:absolute;left:40px;top:300px\"><a href=\"/f\" style=\"color:%23222;text-decoration:none\">foxtrot</a></p>\
    </body></html>";

#[tokio::test]
async fn demoted_link_farm_clears_the_density_gate() -> anyhow::Result<()> {
    let harvester = Harvester::launch().await?;
    let snap = harvester.snapshot(LINK_FARM).await?;
    harvester.close().await?;

    assert!(
        snap.labels.iter().any(|l| l.role == "button"),
        "the real button labels: {:?}",
        snap.labels
    );
    assert!(
        !snap.labels.iter().any(|l| l.role == "link"),
        "every camouflaged link demotes: {:?}",
        snap.labels
    );
    assert!(
        snap.label_coverage >= MIN_LABEL_COVERAGE,
        "a demoted-link page must CLEAR the gate, not fail it: {}",
        snap.label_coverage
    );
    // The demoted links still mask — six ignore boxes in the link column.
    let masked = snap
        .ignore
        .iter()
        .filter(|b| b.x >= 30.0 && b.x <= 100.0 && b.y >= 90.0 && b.y <= 340.0)
        .count();
    assert!(masked >= 6, "all six demoted links must be ignore-masked: {:?}", snap.ignore);
    Ok(())
}

/// C.5 lever: with restore_pointer_links, the same link farm keeps every
/// camouflaged link as a LABEL — nothing demotes, nothing masks, and the
/// page still clears the gate (restored labels are covered surface too).
#[tokio::test]
async fn restore_lever_keeps_pointer_links_as_labels() -> anyhow::Result<()> {
    let mut harvester = Harvester::launch().await?;
    harvester.restore_pointer_links = true;
    let snap = harvester.snapshot(LINK_FARM).await?;
    harvester.close().await?;

    let links = snap.labels.iter().filter(|l| l.role == "link").count();
    assert_eq!(links, 6, "all six camouflaged links stay labeled: {:?}", snap.labels);
    let masked = snap
        .ignore
        .iter()
        .filter(|b| b.x >= 30.0 && b.x <= 100.0 && b.y >= 90.0 && b.y <= 340.0)
        .count();
    assert_eq!(masked, 0, "restored links must not double as ignore-masks: {:?}", snap.ignore);
    assert!(
        snap.label_coverage >= MIN_LABEL_COVERAGE,
        "a restored-link page must clear the gate: {}",
        snap.label_coverage
    );
    Ok(())
}
