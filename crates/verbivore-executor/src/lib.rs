//! Generic executor: runs verb records (data, never generated code) as deterministic
//! CDP actions. Custom-action registry is the escape hatch for behavior that data
//! can't express — the schema stays a flat sequence, control flow stays Rust.
