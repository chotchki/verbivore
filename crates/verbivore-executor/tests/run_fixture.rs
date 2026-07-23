//! Executor e2e against the committed noisy fixture: a hand-authored verb
//! runs deterministically, and every breakage class the repair loop depends
//! on comes back TYPED, not stringly.

use verbivore_executor::{Breakage, ExecutionContext, Executor, RunVerdict};
use verbivore_verb::{
    Action, Assertion, EffectExpectation, FORMAT_VERSION, Provenance, RenderingVariant, Selector,
    Step, TargetSpec, VerbRecord, VerbStatus,
};

fn fixture_url() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/noisy.html")
        .canonicalize()
        .expect("fixture exists");
    format!("file://{}?v=1", root.display())
}

fn button(name: &str) -> Selector {
    Selector { role: "button".into(), name: Some(name.into()), nth: None }
}

fn click(name: &str, intent: &str, expect: EffectExpectation) -> Step {
    Step {
        action: Action::Click,
        target: Some(TargetSpec {
            container: None,
            intent: intent.into(),
            selector: button(name),
        }),
        text: None,
        expect,
    }
}

fn record(steps: Vec<Step>, assertions: Vec<Assertion>) -> VerbRecord {
    let evidence = steps.iter().map(|_| None).collect();
    VerbRecord {
        format_version: FORMAT_VERSION,
        id: "toggle-details".into(),
        intent: "toggle the deployment details".into(),
        app: "fixture".into(),
        start_url: fixture_url(),
        status: VerbStatus::Accepted,
        steps,
        assertions,
        provenance: Provenance {
            created_at: "2026-07-23T00:00:00Z".into(),
            source_url: fixture_url(),
            grounded_by: "hand-authored".into(),
            notes: None,
        },
        variants: vec![RenderingVariant {
            context: ExecutionContext::default().rendering,
            evidence,
        }],
    }
}

#[tokio::test]
async fn accepted_verb_runs_to_passed() -> anyhow::Result<()> {
    let record = record(
        vec![
            click("Toggle details", "details toggle", EffectExpectation::Change),
            click("Do nothing", "the inert button", EffectExpectation::NoChange),
        ],
        vec![
            Assertion::ElementPresent { container: None, selector: button("Add row") },
            Assertion::UrlContains { needle: "noisy.html".into() },
        ],
    );
    let executor = Executor::launch().await?;
    let run = executor.run(&record, &ExecutionContext::default()).await?;
    executor.close().await?;

    assert_eq!(run.verdict, RunVerdict::Passed, "run: {run:?}");
    assert_eq!(run.steps.len(), 2);
    assert!(run.steps[0].signals.dom_mutations > 0, "toggle must mutate");
    assert_eq!(run.steps[1].signals.dom_mutations, 0, "noop must stay silent");
    assert_ne!(
        run.steps[0].before_png, run.steps[0].after_png,
        "toggle must repaint"
    );
    Ok(())
}

#[tokio::test]
async fn drifted_selector_reports_target_not_found() -> anyhow::Result<()> {
    let record = record(
        vec![click("No Such Button", "ghost", EffectExpectation::Change)],
        vec![],
    );
    let executor = Executor::launch().await?;
    let run = executor.run(&record, &ExecutionContext::default()).await?;
    executor.close().await?;

    match run.verdict {
        RunVerdict::Broken { breakage: Breakage::TargetNotFound { step: 0, selector } } => {
            assert_eq!(selector.name.as_deref(), Some("No Such Button"));
        }
        other => panic!("expected TargetNotFound, got {other:?}"),
    }
    Ok(())
}

#[tokio::test]
async fn ungrounded_rendering_is_a_repair_trigger_not_a_guess() -> anyhow::Result<()> {
    let record = record(
        vec![click("Toggle details", "details toggle", EffectExpectation::Change)],
        vec![],
    );
    let mut ctx = ExecutionContext::default();
    ctx.rendering.dpr = 2.0; // no variant grounded this rendering
    let executor = Executor::launch().await?;
    let run = executor.run(&record, &ctx).await?;
    executor.close().await?;

    assert_eq!(
        run.verdict,
        RunVerdict::Broken { breakage: Breakage::NoVariantForContext },
        "must refuse before touching the page"
    );
    assert!(run.steps.is_empty());
    Ok(())
}

#[tokio::test]
async fn candidate_records_refuse_outside_review_mode() -> anyhow::Result<()> {
    let mut candidate = record(
        vec![click("Toggle details", "details toggle", EffectExpectation::Change)],
        vec![],
    );
    candidate.status = VerbStatus::Candidate;
    let executor = Executor::launch().await?;
    let refused = executor.run(&candidate, &ExecutionContext::default()).await?;
    let allowed = executor
        .run(&candidate, &ExecutionContext { allow_candidates: true, ..Default::default() })
        .await?;
    executor.close().await?;

    assert!(matches!(
        refused.verdict,
        RunVerdict::Broken { breakage: Breakage::NotAccepted { .. } }
    ));
    assert_eq!(allowed.verdict, RunVerdict::Passed, "run: {allowed:?}");
    Ok(())
}
