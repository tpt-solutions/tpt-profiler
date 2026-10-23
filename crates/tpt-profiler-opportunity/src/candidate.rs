//! Candidates: the engine's input (spec §25).
//!
//! A [`Candidate`] is one detected pattern with its evidence, produced by
//! mapping analyzer output (static findings, dependency findings) onto
//! model types. The engine turns candidates into scored
//! [`tpt_profiler_model::Opportunity`]s — or rejects them when evidence is insufficient
//! (spec §3.1.12).

use std::string::String;
use std::vec::Vec;

use tpt_profiler_model::{Category, Evidence, Measurement};

/// One detected pattern awaiting conversion into an opportunity.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Repository the pattern was found in.
    pub repository: String,
    /// Optimisation category (spec §26).
    pub category: Category,
    /// Component path (module path or file) the pattern was found in.
    pub component: String,
    /// Short description of the pattern.
    pub description: String,
    /// Backing evidence, ranked per spec §29. Empty evidence means the
    /// candidate will be rejected (spec §3.1.12).
    pub evidence: Vec<Evidence>,
    /// Whether the pattern sits in floating-point code (spec §55).
    pub numerical_context: bool,
    /// Optional generality hint, 1–10 (spec §32); `None` lets the engine
    /// use the category default.
    pub generality_hint: Option<u8>,
}

impl Candidate {
    /// A candidate with no evidence yet.
    #[must_use]
    pub fn new(
        repository: impl Into<String>,
        category: Category,
        component: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            repository: repository.into(),
            category,
            component: component.into(),
            description: description.into(),
            evidence: Vec::new(),
            numerical_context: false,
            generality_hint: None,
        }
    }

    /// Runtime-evidence suppression check (spec §2): when a measurement
    /// in the evidence says this code is rarely executed (a
    /// coverage/ratio/percent metric below 1%), the candidate must not
    /// become a recommendation.
    #[must_use]
    pub fn rarely_executed(&self) -> bool {
        const RARELY_EXECUTED_THRESHOLD: f64 = 0.01;
        self.evidence.iter().any(|evidence| {
            evidence
                .measurement
                .as_ref()
                .is_some_and(|measurement: &Measurement| {
                    let metric = measurement.metric.as_str();
                    (metric.contains("coverage")
                        || metric.contains("percent")
                        || metric.contains("ratio"))
                        && measurement.value < RARELY_EXECUTED_THRESHOLD
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_profiler_model::{EvidenceKind, MeasurementSource};

    #[test]
    fn rarely_executed_code_is_detectable_from_runtime_evidence() {
        let mut candidate = Candidate::new("repo", Category::Simd, "hot::path", "loop");
        assert!(!candidate.rarely_executed(), "no measurement at all");

        let cold = Measurement::runtime(
            "profile_coverage",
            0.002,
            tpt_profiler_model::Unit::Ratio,
            MeasurementSource::Perf,
            0,
        );
        candidate.evidence.push(Evidence {
            kind: EvidenceKind::RuntimeProfile,
            description: String::from("cold path"),
            location: None,
            measurement: Some(cold),
        });
        assert!(candidate.rarely_executed());

        let warm = Measurement::runtime(
            "profile_coverage",
            0.4,
            tpt_profiler_model::Unit::Ratio,
            MeasurementSource::Perf,
            0,
        );
        candidate.evidence.clear();
        candidate.evidence.push(Evidence {
            kind: EvidenceKind::RuntimeProfile,
            description: String::from("warm path"),
            location: None,
            measurement: Some(warm),
        });
        assert!(!candidate.rarely_executed());
    }
}
