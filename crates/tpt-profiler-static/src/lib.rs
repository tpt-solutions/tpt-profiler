//! Static analysis engine for `tpt-profiler` (spec §9–13).
//!
//! `syn`-based, deterministic source analysis producing standardised
//! evidence (spec §67): allocation patterns, clone analysis, data-structure
//! mismatches, loop opportunities, SIMD candidates, and parallelism
//! candidates — each with evidence and confidence, never blind flagging
//! (spec §9).
//!
//! Structure:
//!
//! * [`finding::Finding`] — the standardised detection unit.
//! * [`engine::Analyzer`] — the analyzer trait; [`engine::StaticEngine`]
//!   is the registry and driver.
//! * [`detectors`] — built-in detectors.
//! * [`context::FileContext`] — per-file unsafe/numerical/suppression
//!   context (spec §54, §55).
//!
//! Limits (documented per spec §10): `syn`-level analysis has no type
//! information, so Arc/Rc recognition and collection kinds are
//! name-based heuristics. Confidence never exceeds `Medium`, and
//! `// tpt-profiler:ignore` comments suppress false positives.

pub mod context;
pub mod detectors;
pub mod engine;
pub mod finding;

pub use context::{FileContext, IGNORE_MARKER, IGNORE_NEXT_MARKER};
pub use engine::{Analyzer, StaticEngine, StaticReport};
pub use finding::Finding;

/// Runs the default detector set over one source string; convenience for
/// embedders and tests.
#[must_use]
pub fn analyze_source(
    file_name: &str,
    source: &str,
) -> (Vec<Finding>, Vec<tpt_profiler_model::error::ParseError>) {
    StaticEngine::with_default_analyzers().analyze_str(file_name, source)
}
