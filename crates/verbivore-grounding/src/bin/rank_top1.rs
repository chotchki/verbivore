//! The 2.9 gate measurement: top-1 grounding accuracy over a harvested dataset.
//! For every named element, phrase the intent the way a test author would
//! ("the <name> <role-word>") and ask the ranker to find it among ALL of the
//! page's elements. No detector involved — this is the a11y-candidates path.
//!
//!   cargo run --release -p verbivore-grounding --bin rank-top1 -- <dataset_dir>
//!
//! Same-name-same-role duplicates count as misses on purpose: a human phrase
//! can't disambiguate them either — that's what container scoping is for.

use verbivore_dataset::Dataset;
use verbivore_grounding::rank::rank;

/// A test author says "button", not "menuitemcheckbox".
fn role_word(role: &str) -> &str {
    match role {
        "searchbox" => "search box",
        "textbox" => "field",
        "combobox" => "dropdown",
        "menuitem" | "menuitemcheckbox" | "menuitemradio" => "menu item",
        "spinbutton" => "spinner",
        other => other,
    }
}

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("dataset dir");
    let ds = Dataset::open(dir)?;

    let (mut hits, mut total, mut candidates_sum) = (0usize, 0usize, 0usize);
    for id in ds.sample_ids()? {
        let meta = ds.meta(&id)?;
        for (target, label) in meta.labels.iter().enumerate() {
            let Some(name) = label.name.as_deref().filter(|n| !n.trim().is_empty()) else {
                continue;
            };
            let intent = format!("the {} {}", name, role_word(&label.role));
            let ranked = rank(&intent, &meta.labels);
            total += 1;
            candidates_sum += meta.labels.len();
            if ranked.first().is_some_and(|r| r.index == target) {
                hits += 1;
            }
        }
    }

    println!(
        "top-1: {:.3} ({hits}/{total} named elements, avg {:.0} candidates/page)",
        hits as f64 / total.max(1) as f64,
        candidates_sum as f64 / total.max(1) as f64
    );
    Ok(())
}
