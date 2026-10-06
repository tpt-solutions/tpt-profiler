//! Shared data model for `tpt-profiler`.
//!
//! This crate is the dependency-free leaf of the workspace (see
//! `DESIGN.md` §2): every other crate may depend on it, it depends on
//! nothing else in the workspace.
//!
//! It is `no_std` + [`alloc`] by default (spec §73); the optional `std`
//! feature enables OS-specific helpers, and the optional `serde` feature
//! (with `default-features = false`) adds serialisation to the persisted
//! and emitted types.
//!
//! Contents:
//!
//! * [`error`] — the structured error taxonomy (spec §74),
//!   which distinguishes *"no opportunity detected"* (not an error) from
//!   actual failures via [`outcome::AnalysisOutcome`].
//! * [`schema`] — versioned envelope for all persisted/emitted JSON
//!   (spec §69).
//! * [`hash`] — stable FNV-1a hashing used for deterministic IDs.
//! * [`id`] — deterministic, reproducible opportunity IDs (spec §25).
//! * [`category`] — optimisation categories (spec §26).
//! * [`scales`] — impact, confidence, effort, risk, generality, priority
//!   (spec §27–§32).
//! * [`location`] — source locations with unsafe-region tags (spec §54).
//! * [`workload`] — explicit workload model (spec §23).
//! * [`environment`] — execution-environment capture (spec §48, §49).
//! * [`measurement`] — observed facts (spec §24, §51).
//! * [`evidence`] — ranked evidence hierarchy (spec §29).
//! * [`hierarchy`] — repository/project/crate/module/function model
//!   (spec §6, §7).
//! * [`lifecycle`] — opportunity lifecycle states (spec §38).
//! * [`opportunity`] — the fundamental unit of the profiler (spec §25).
//! * [`run`] — profile runs (spec §6, §49).

#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;

pub mod category;
pub mod environment;
pub mod error;
pub mod evidence;
pub mod hash;
pub mod hierarchy;
pub mod id;
pub mod lifecycle;
pub mod location;
pub mod measurement;
pub mod opportunity;
pub mod outcome;
pub mod run;
pub mod scales;
pub mod schema;
pub mod workload;

pub use category::Category;
pub use environment::Environment;
pub use evidence::{Evidence, EvidenceKind, confidence_ceiling, strongest_rank};
pub use hierarchy::{
    CrateInfo, DependencyInfo, DependencyKind, FunctionInfo, ModuleInfo, Project, Repository,
};
pub use id::OpportunityId;
pub use lifecycle::LifecycleState;
pub use location::SourceLocation;
pub use measurement::{FindingKind, Measurement, MeasurementSource, Unit, UnixSeconds};
pub use opportunity::{
    Interpretation, Opportunity, Recommendation, Relationship, RelationshipKind,
};
pub use outcome::AnalysisOutcome;
pub use run::ProfileRun;
pub use scales::{
    Confidence, Effort, Generality, Impact, Priority, Risk, RiskDimensions, RiskScore, ScaleError,
};
pub use schema::{SCHEMA_VERSION, SchemaEnvelope};
pub use workload::Workload;

/// Version of this crate, as reported in run records and schema
/// envelopes.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
