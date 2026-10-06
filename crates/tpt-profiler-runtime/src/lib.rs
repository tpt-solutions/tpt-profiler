//! Runtime profiling for `tpt-profiler` (spec §8.2, §23, §50).
//!
//! Runs configured workloads, collects wall/CPU time, handles timeouts and
//! cancellation, and emits measurements with full environment context.
//! Statistical rigour (repeats, outliers, confidence) is mandatory: a
//! single execution is never definitive.
//!
//! **Status:** skeleton. Lands with the MVP (spec §62).
