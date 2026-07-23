//! The repair loop: run, catch the breakage, re-ground from the break scene,
//! patch the record, verify. Repair on DOM pages ranks the step's INTENT over
//! the live a11y labels — no vision model in the loop (vision earns its keep
//! on canvas and at authoring time), which keeps repair deterministic and
//! this crate framework-free.
//!
//! A repaired record drops back to Candidate: grounding changed, a human
//! re-accepts (the reviewable diff is the record diff on disk — one json per
//! verb exists for exactly this moment).

use anyhow::{Context, Result};
use serde::Serialize;
use verbivore_dataset::rank::rank;
use verbivore_verb::{Selector, StepEvidence, VerbStatus, VerbStore, selector_for};

use crate::{Breakage, ExecutionContext, Executor, RunVerdict};

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RepairOutcome {
    /// The verb runs clean; nothing to do.
    NothingToRepair,
    Repaired {
        step: usize,
        old: Selector,
        new: Selector,
        /// The patched record re-ran to Passed.
        verified: bool,
    },
    /// Breakage repair can't address by re-grounding: dead elements
    /// (EffectSilence needs re-authoring, not a new selector), missing
    /// variants, failed assertions — or no candidate matched the intent.
    Unrepairable { breakage: Breakage },
}

/// One repair pass over a stored verb. Loads, runs (candidates allowed —
/// repair often iterates on already-demoted records), patches on a
/// target-resolution breakage, saves, verifies.
pub async fn repair_verb(
    executor: &Executor,
    store: &VerbStore,
    app: &str,
    id: &str,
    ctx: &ExecutionContext,
) -> Result<RepairOutcome> {
    let mut record = store.load(app, id)?;
    let mut ctx = ctx.clone();
    ctx.allow_candidates = true;

    let run = executor.run(&record, &ctx).await?;
    let breakage = match run.verdict {
        RunVerdict::Passed => return Ok(RepairOutcome::NothingToRepair),
        RunVerdict::Broken { breakage } => breakage,
    };
    let step = match &breakage {
        Breakage::TargetNotFound { step, .. } | Breakage::AmbiguousTarget { step, .. } => *step,
        _ => return Ok(RepairOutcome::Unrepairable { breakage }),
    };
    let scene = run
        .break_scene
        .context("target breakage without a break scene")?;

    let target = record.steps[step]
        .target
        .as_mut()
        .context("target breakage on an untargeted step")?;
    let ranked = rank(&target.intent, &scene.labels);
    let Some(best) = ranked.first() else {
        return Ok(RepairOutcome::Unrepairable { breakage });
    };
    let label = &scene.labels[best.index];
    let old = std::mem::replace(&mut target.selector, selector_for(label, &scene.labels));
    let new = target.selector.clone();

    // Fresh grounding evidence for the rendering that broke; other variants
    // keep their (now suspect) evidence — they re-verify on their own runs.
    for variant in &mut record.variants {
        if variant.context == ctx.rendering {
            variant.evidence[step] = Some(StepEvidence {
                bbox: label.bbox,
                score: best.score as f64,
            });
        }
    }
    // Grounding changed -> back through review.
    record.status = VerbStatus::Candidate;
    store.save(&record)?;

    let verified = matches!(
        executor.run(&record, &ctx).await?.verdict,
        RunVerdict::Passed
    );
    Ok(RepairOutcome::Repaired { step, old, new, verified })
}
