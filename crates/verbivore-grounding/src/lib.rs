//! Element grounding: screenshot in, interactive elements out (bbox + role + label).
//! Burn detector, trained from scratch on harvested data; runs at authoring/repair
//! time, never in the runtime loop (canvas verbs are the one exception).

pub mod data;
pub mod decode;
pub mod eval;
pub mod loss;
pub mod model;
/// Re-export: rank moved to the burn-free dataset crate so the executor's
/// repair loop can use it without an ML-framework dep. Vision stays here.
pub use verbivore_dataset::rank;
pub mod train;
