//! The v1 loop, end to end on the committed fixture: crawl proposes
//! candidates, review accepts what runs and rejects what doesn't (the
//! sabotage self-report), and an accepted verb executes deterministically.

use verbivore_executor::accept::{AcceptOutcome, review_and_accept};
use verbivore_executor::{Breakage, ExecutionContext, Executor, RunVerdict};
use verbivore_generator::crawl::{DEFAULT_DENY, crawl};
use verbivore_harvester::Harvester;
use verbivore_verb::{VerbStatus, VerbStore};

fn fixture_url() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/noisy.html")
        .canonicalize()
        .expect("fixture exists");
    format!("file://{}?v=1", root.display())
}

#[tokio::test]
async fn crawl_review_execute_and_sabotage_self_report() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let store = VerbStore::open(dir.path())?;
    let deny: Vec<String> = DEFAULT_DENY.iter().map(|s| s.to_string()).collect();

    // Crawl: one page, candidates only.
    let harvester = Harvester::launch().await?;
    let report = crawl(&harvester, &store, "fixture", &fixture_url(), 3, 20, &deny).await?;
    harvester.close().await?;
    assert_eq!(report.pages, 1, "file:// fixture has no frontier");
    assert!(report.proposed >= 4, "five buttons minus dupes: {}", report.proposed);
    let toggle = store.load("fixture", "press-toggle-details")?;
    assert_eq!(toggle.status, VerbStatus::Candidate);
    toggle.validate()?;

    let executor = Executor::launch().await?;
    let ctx = ExecutionContext::default();

    // Review: the live button earns Accepted.
    let outcome = review_and_accept(&executor, &store, "fixture", "press-toggle-details", &ctx).await?;
    assert!(matches!(outcome, AcceptOutcome::Accepted), "{outcome:?}");
    assert_eq!(
        store.load("fixture", "press-toggle-details")?.status,
        VerbStatus::Accepted
    );

    // Sabotage self-report: the dead button proposes like any other, and
    // review is where it dies — typed, not silently.
    let outcome = review_and_accept(&executor, &store, "fixture", "press-do-nothing", &ctx).await?;
    match outcome {
        AcceptOutcome::Rejected { breakage: Breakage::EffectSilence { step: 0 } } => {}
        other => panic!("dead click must be rejected as EffectSilence: {other:?}"),
    }
    assert_eq!(
        store.load("fixture", "press-do-nothing")?.status,
        VerbStatus::Candidate,
        "rejected candidates stay candidates"
    );

    // The accepted verb runs deterministically outside review mode.
    let run = executor
        .run(&store.load("fixture", "press-toggle-details")?, &ctx)
        .await?;
    executor.close().await?;
    assert_eq!(run.verdict, RunVerdict::Passed, "run: {run:?}");
    Ok(())
}
