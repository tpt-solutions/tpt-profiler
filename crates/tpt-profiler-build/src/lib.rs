//! Build-time analysis for `tpt-profiler` (spec §8.6, §20).
//!
//! Clean and incremental compile times, per-crate and per-dependency
//! build cost, proc-macro/build-script overhead, and feature-combination
//! cost (via `cargo --timings` and friends).
//!
//! **Status:** skeleton. Lands in Phase 2 (spec §63).
