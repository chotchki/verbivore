//! Diagnostic (temporary): label yield + coverage across the new corpus apps.
use verbivore_harvester::Harvester;

#[tokio::test]
#[ignore = "needs the corpus running"]
async fn new_apps_label_yield() -> anyhow::Result<()> {
    let harvester = Harvester::launch().await?;
    for (name, url) in [
        ("bootstrap", "http://localhost:42009/checkout/"),
        ("uswds", "http://localhost:42010/"),
        ("apg-combobox", "http://localhost:42011/content/patterns/combobox/examples/combobox-select-only.html"),
        ("materialize", "http://localhost:42012/"),
        ("dokuwiki", "http://localhost:42013/"),
        ("bulma", "http://localhost:42014/"),
        ("fomantic", "http://localhost:42015/"),
        ("pico", "http://localhost:42016/"),
    ] {
        let snap = harvester.snapshot(url).await?;
        let mut roles: Vec<&str> = snap.labels.iter().map(|l| l.role.as_str()).collect();
        roles.sort();
        roles.dedup();
        println!(
            "{name:14} labels={:3} ignore={:3} coverage={:.2} roles={roles:?}",
            snap.labels.len(),
            snap.ignore.len(),
            snap.label_coverage
        );
    }
    harvester.close().await?;
    Ok(())
}
