//! Core library for `tpt-profiler`.
//!
//! This crate owns the orchestrator pipeline (spec §5):
//!
//! ```text
//! discovery → analysis → evidence → opportunities → priority → report
//! ```
//!
//! and the analyzer registry (spec §67). It is the only crate allowed to
//! depend on all analysis crates; embedders should depend on this crate
//! rather than the internals.
//!
//! **Status:** skeleton. The pipeline lands with the MVP (spec §62);
//! the data model is already available via [`model`].

pub use tpt_profiler_model as model;
