//! One intent phrase -> one candidate record: open the page, rank the intent
//! over its live elements, snap the winner into a verb. The single-phrase
//! entrance for when a human knows the task they want.

use anyhow::{Context, Result, ensure};
use verbivore_dataset::rank::rank;
use verbivore_harvester::{Harvester, Variation};
use verbivore_verb::{
    Action, EffectExpectation, FORMAT_VERSION, Provenance, RenderingVariant, Step, StepEvidence,
    TargetSpec, VerbRecord, VerbStatus, VerbStore,
};

use crate::propose::{selector_of, slug};

pub async fn generate(
    harvester: &Harvester,
    store: &VerbStore,
    app: &str,
    url: &str,
    intent: &str,
) -> Result<VerbRecord> {
    let variation = Variation::default();
    let page = harvester.open_page(url, &variation).await?;
    let elements = harvester.page_map(&page, &variation).await?;
    page.close().await.ok();
    ensure!(!elements.is_empty(), "no interactive elements on {url}");

    let flat: Vec<_> = elements.iter().map(|e| e.label.clone()).collect();
    let ranked = rank(intent, &flat);
    let best = ranked.first().context("nothing ranked for the intent")?;
    let element = &elements[best.index];

    let record = VerbRecord {
        format_version: FORMAT_VERSION,
        id: slug(intent),
        intent: intent.to_string(),
        app: app.to_string(),
        start_url: url.to_string(),
        status: VerbStatus::Candidate,
        steps: vec![Step {
            action: Action::Click,
            target: Some(TargetSpec {
                container: element.container.as_ref().and_then(|c| {
                    c.name.as_ref().map(|n| format!("{} {}", n.to_lowercase(), c.role))
                }),
                intent: intent.to_string(),
                selector: selector_of(element, &elements),
            }),
            text: None,
            expect: EffectExpectation::Change,
        }],
        assertions: Vec::new(),
        provenance: Provenance {
            created_at: crate::now_iso(),
            source_url: url.to_string(),
            grounded_by: format!("rank-a11y@v1 score={:.2}", best.score),
            notes: None,
        },
        variants: vec![RenderingVariant {
            context: crate::default_rendering(),
            evidence: vec![Some(StepEvidence {
                bbox: element.label.bbox,
                score: best.score as f64,
            })],
        }],
    };
    store.save(&record)?;
    Ok(record)
}
