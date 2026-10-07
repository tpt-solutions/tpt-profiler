//! Opportunity engine for `tpt-profiler` (spec §25–35, §51).
//!
//! Converts candidates (pattern + evidence) into ranked, explained
//! opportunities:
//!
//! * [`candidate::Candidate`] — the engine's input.
//! * [`estimate`] — impact/confidence/effort/risk/generality heuristics
//!   (§28–§32), bounded by the §29 evidence hierarchy.
//! * [`score`] — the configurable priority formula (§27, not an API
//!   contract) with an explainable trace.
//! * [`engine`] — rejection rules (§3.1.12 insufficient evidence, §2
//!   rarely-executed code), aggregation (§58), relationship edges
//!   (§35), `top_n` strategies (§59), and `explain` rendering.
//!
//! "No opportunity" is a valid [`AnalysisOutcome`], never an error
//! (spec §74).

pub mod candidate;
pub mod engine;
pub mod estimate;
pub mod score;

pub use candidate::Candidate;
pub use engine::{
    AGGREGATION_THRESHOLD, Strategy, aggregate, build_opportunities, explain, score_trace,
    sort_by_strategy, top_n,
};
pub use estimate::{Estimates, estimate as estimate_dimensions};
pub use score::{ScoringTrace, ScoringWeights, compute_priority};
