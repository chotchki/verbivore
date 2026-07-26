//! C.1 contract: the affordance collector sees what the DOM knows about
//! where actions could land — including addEventListener geometry (the 6.1
//! blind spot, closed here) and document-scoped handlers as AMBIENT.

use verbivore_harvester::Harvester;
use verbivore_dataset::{AffordanceChannel, AffordanceSource};

/// One of everything: a cursor-pointer div, a text input, a button whose
/// click handler exists ONLY via addEventListener (invisible to attribute
/// scans), a scrollable overflow region and a document-level keydown.
const FIXTURE: &str = "data:text/html,<html><body style=\"margin:0\">\
    <div id=\"ptr\" style=\"position:absolute;left:40px;top:20px;width:100px;height:30px;cursor:pointer\">fake</div>\
    <input id=\"txt\" style=\"position:absolute;left:40px;top:80px;width:160px;height:24px\">\
    <button id=\"wired\" style=\"position:absolute;left:40px;top:140px;width:120px;height:32px\">wired</button>\
    <div id=\"scroller\" style=\"position:absolute;left:40px;top:200px;width:200px;height:60px;overflow-y:scroll\">\
      <div style=\"height:400px\">tall content</div></div>\
    <script>\
      document.getElementById('wired').addEventListener('click', () => {});\
      document.addEventListener('keydown', () => {});\
    </script>\
    </body></html>";

#[tokio::test]
async fn collects_all_evidence_kinds() -> anyhow::Result<()> {
    let harvester = Harvester::launch().await?;
    let snap = harvester.snapshot(FIXTURE).await?;
    harvester.close().await?;

    let ev = &snap.affordance;
    let has = |ch: AffordanceChannel, src: AffordanceSource| {
        ev.iter().any(|e| e.channel == ch && e.source == src)
    };

    assert!(
        has(AffordanceChannel::Pointer, AffordanceSource::CursorPointer),
        "cursor:pointer div must show as pointer evidence: {ev:?}"
    );
    assert!(
        has(AffordanceChannel::Keyboard, AffordanceSource::NativeTag)
            && has(AffordanceChannel::Pointer, AffordanceSource::NativeTag),
        "the input is both clickable and typeable: {ev:?}"
    );
    assert!(
        has(AffordanceChannel::Scroll, AffordanceSource::Scrollable),
        "the overflow region must show as scroll evidence: {ev:?}"
    );

    // The 6.1 blind spot, closed: a listener attached purely via
    // addEventListener localizes to ITS button's geometry.
    let wired = ev
        .iter()
        .find(|e| {
            e.channel == AffordanceChannel::Pointer && e.source == AffordanceSource::Listener
        })
        .expect("addEventListener click must surface as Listener evidence");
    assert!(
        wired.bbox.y > 130.0 && wired.bbox.y < 180.0 && wired.bbox.x > 30.0,
        "listener evidence must carry the button's bbox, got {:?}",
        wired.bbox
    );

    // Document-scoped keydown: real evidence, no localization -> AMBIENT
    // with a viewport-sized bbox.
    let ambient = ev
        .iter()
        .find(|e| {
            e.channel == AffordanceChannel::Keyboard && e.source == AffordanceSource::Ambient
        })
        .expect("document keydown must surface as ambient keyboard evidence");
    assert!(
        ambient.bbox.w >= 1000.0 && ambient.bbox.x == 0.0,
        "ambient evidence spans the viewport, got {:?}",
        ambient.bbox
    );
    Ok(())
}
