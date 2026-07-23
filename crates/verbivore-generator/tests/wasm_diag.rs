//! Canvas walk prototype against the fab-scad editor: no DOM to chain, no
//! urls to compare — grid points ARE the candidates, position IS the
//! identity, and the VISUAL channel judges which regions respond. This is
//! the canvas-verbs recipe run as a diagnostic.
use verbivore_generator::probe::{NavCandidate, probe_order};
use verbivore_harvester::{Harvester, Variation, effect_capture, input};

const EDITOR: &str = "https://beta.hotchkiss.io:8443/3d/editor";

#[tokio::test]
#[ignore = "remote diagnostic"]
async fn fabscad_canvas_liveness_map() -> anyhow::Result<()> {
    let harvester = Harvester::launch().await?;
    let variation = Variation::default();

    // Grid candidates over the viewport: identical tokens (one canvas, one
    // url), so probe_order degenerates to pure spatial farthest-first.
    let mut candidates = Vec::new();
    for gy in 0..4 {
        for gx in 0..5 {
            candidates.push(NavCandidate {
                tokens: vec!["canvas".into()],
                selector: "canvas".into(),
                x: 1280.0 * (gx as f64 + 0.5) / 5.0,
                y: 800.0 * (gy as f64 + 0.5) / 4.0,
            });
        }
    }

    let mut live = 0;
    for &i in probe_order(&candidates).iter().take(10) {
        let c = &candidates[i];
        let page = harvester.open_page(EDITOR, &variation).await?;
        harvester.settle_render(&page).await?;
        tokio::time::sleep(std::time::Duration::from_secs(4)).await; // wasm boot
        let before = effect_capture::shot(&page).await?;
        input::click_at(&page, c.x, c.y).await?;
        tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        let after = effect_capture::shot(&page).await?;
        page.close().await.ok();
        let ssim = verbivore_effect::mssim_png(&before, &after)?;
        let verdict = if ssim < 0.995 { live += 1; "LIVE" } else { "dead" };
        println!("({:4.0},{:4.0}) ssim={ssim:.4} {verdict}", c.x, c.y);
    }
    println!("{live}/10 grid regions respond to clicks");
    harvester.close().await?;
    Ok(())
}
