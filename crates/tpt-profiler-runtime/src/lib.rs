//! Runtime profiling for `tpt-profiler` (spec §8.2, §23, §50).
//!
//! Runs configured workloads, collects wall time, handles timeouts and
//! cancellation, and emits measurements with full environment context.
//! Statistical rigour (repeats, warm-up, outliers, confidence) is
//! mandatory: a single execution is never definitive.

pub mod measure;
pub mod process;
pub mod stats;

pub use measure::{RunnableWorkload, WorkloadResult, measure_workload, to_measurements};
pub use process::{CancelToken, RunOutcome, run};
pub use stats::Summary;
