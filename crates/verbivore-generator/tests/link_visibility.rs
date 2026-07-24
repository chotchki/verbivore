//! Diagnostic: what fraction of links are visually EVIDENT vs pointer-only?
//! A link whose style matches its parent text (no color shift, no underline,
//! no weight change) is invisible in a static screenshot at any resolution —
//! labeling it teaches the detector noise.
use verbivore_harvester::{Harvester, Variation};

const SCAN: &str = r#"
(() => {
    let evident = 0, invisible = 0;
    for (const a of document.querySelectorAll('a[href]')) {
        const r = a.getBoundingClientRect();
        if (r.width < 4 || r.height < 4) continue;
        const s = getComputedStyle(a);
        const p = a.parentElement ? getComputedStyle(a.parentElement) : s;
        const distinct = s.color !== p.color
            || s.textDecorationLine.includes('underline')
            || (parseInt(s.fontWeight) >= 600 && parseInt(p.fontWeight) < 600)
            || s.backgroundColor !== p.backgroundColor;
        if (distinct) evident++; else invisible++;
    }
    return [evident, invisible];
})()
"#;

#[tokio::test]
#[ignore = "needs the corpus running"]
async fn invisible_link_rate() -> anyhow::Result<()> {
    let harvester = Harvester::launch().await?;
    let mut total = (0i64, 0i64);
    for (name, url) in [
        ("wordpress", "http://localhost:42003/"),
        ("mediawiki", "http://localhost:42004/index.php/Main_Page"),
        ("gitea", "http://localhost:42002/explore/repos"),
        ("ghost", "http://localhost:42007/"),
        ("bootstrap", "http://localhost:42009/"),
        ("grafana", "http://localhost:42001/dashboards"),
    ] {
        let page = harvester.open_page(url, &Variation::default()).await?;
        harvester.settle_render(&page).await?;
        let counts: Vec<i64> = page.evaluate(SCAN).await?.into_value()?;
        page.close().await.ok();
        let rate = counts[1] as f64 / (counts[0] + counts[1]).max(1) as f64;
        println!("{name:10} evident={:4} invisible={:4} ({:.0}% invisible)", counts[0], counts[1], rate * 100.0);
        total.0 += counts[0];
        total.1 += counts[1];
    }
    println!("TOTAL      evident={:4} invisible={:4} ({:.0}% invisible)",
        total.0, total.1, total.1 as f64 / (total.0 + total.1).max(1) as f64 * 100.0);
    harvester.close().await?;
    Ok(())
}
