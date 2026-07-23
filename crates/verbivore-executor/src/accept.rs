//! The accept flow: a candidate earns Accepted by RUNNING, not by looking
//! plausible. Review executes the record (candidates allowed, that's the
//! point) and only a Passed run flips the status — a candidate that breaks
//! stays a candidate, with the typed breakage as the review note.

use anyhow::Result;
use serde::Serialize;
use verbivore_verb::{VerbStatus, VerbStore};

use crate::{Breakage, ExecutionContext, Executor, RunVerdict};

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AcceptOutcome {
    Accepted,
    AlreadyAccepted,
    /// The review run broke; status untouched.
    Rejected { breakage: Breakage },
}

pub async fn review_and_accept(
    executor: &Executor,
    store: &VerbStore,
    app: &str,
    id: &str,
    ctx: &ExecutionContext,
) -> Result<AcceptOutcome> {
    let mut record = store.load(app, id)?;
    if record.status == VerbStatus::Accepted {
        return Ok(AcceptOutcome::AlreadyAccepted);
    }
    let mut ctx = ctx.clone();
    ctx.allow_candidates = true;
    match executor.run(&record, &ctx).await?.verdict {
        RunVerdict::Passed => {
            record.status = VerbStatus::Accepted;
            store.save(&record)?;
            Ok(AcceptOutcome::Accepted)
        }
        RunVerdict::Broken { breakage } => Ok(AcceptOutcome::Rejected { breakage }),
    }
}
