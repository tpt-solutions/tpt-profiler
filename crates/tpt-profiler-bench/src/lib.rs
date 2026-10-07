//! Benchmark analysis for `tpt-profiler` (spec §8.3).
//!
//! Discovers Criterion/`cargo bench` harnesses, executes them, and
//! ingests their results as measurements. Absent benches or absent
//! Criterion data are *results*, not crashes (spec §74).
//!
//! Note: `cargo_metadata` remains a dependency for future per-bench
//! target resolution; bench execution goes through `cargo bench` (spec
//! §68: external tools are evidence providers).

pub mod criterion;
pub mod runner;

pub use criterion::{CriterionEstimate, collect_estimates, parse_estimates, to_measurements};
pub use runner::{BenchReport, run_benches};
