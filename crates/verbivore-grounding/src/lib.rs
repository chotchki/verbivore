//! Element grounding: screenshot in, interactive elements out (bbox + role + label).
//! Burn detector, trained from scratch on harvested data; runs at authoring/repair
//! time, never in the runtime loop (canvas verbs are the one exception).

pub mod data;
pub mod decode;
pub mod eval;
pub mod loss;
pub mod model;
pub mod train;
