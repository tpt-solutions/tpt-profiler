//! Benchmark analysis for `tpt-profiler` (spec §8.3).
//!
//! Discovers Criterion/`cargo bench` harnesses, executes them, and ingests
//! their results as measurements. Behaves gracefully (a `ToolUnavailable`
//! error, not a crash) when no benchmark harness exists (spec §74).
//!
//! **Status:** skeleton. Lands with the MVP (spec §62).
