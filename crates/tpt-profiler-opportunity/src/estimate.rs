//! Heuristic estimation of the opportunity dimensions (spec §28–§32).
//!
//! Every estimator is deliberately simple and *documented*: static
//! evidence bounds what the engine may claim, and runtime evidence can
//! raise confidence but never above the §29 ceiling for its tier. These
//! are starting heuristics, tuned over time (spec §52: dismissal data
//! feeds back) — they are not an API contract.

use std::string::String;
use std::vec::Vec;

use tpt_profiler_model::{
    Category, Confidence, Effort, Evidence, Generality, Impact, Risk, RiskDimensions, RiskScore,
    confidence_ceiling, strongest_rank,
};

use crate::candidate::Candidate;

/// Estimated dimensions for one candidate (spec §25).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Estimates {
    /// Expected impact, 1–10 (spec §28).
    pub impact: Impact,
    /// Confidence per the evidence hierarchy (spec §29).
    pub confidence: Confidence,
    /// Estimated effort (spec §30).
    pub effort: Effort,
    /// Risk with dimensions (spec §31).
    pub risk: Risk,
    /// How broadly the fix applies (spec §32).
    pub generality: Generality,
}

/// Estimates all dimensions for a candidate with non-empty evidence.
#[must_use]
pub fn estimate(candidate: &Candidate) -> Estimates {
    let rank = strongest_rank(&candidate.evidence);
    Estimates {
        impact: estimate_impact(rank),
        confidence: estimate_confidence(&candidate.evidence),
        effort: estimate_effort(candidate),
        risk: estimate_risk(candidate),
        generality: estimate_generality(candidate),
    }
}

/// Impact tracks evidence strength: observed regressions transform an
/// opportunity, bare heuristics barely move the needle (spec §28).
#[must_use]
pub fn estimate_impact(strongest_evidence_rank: u8) -> Impact {
    let value = 3 + strongest_evidence_rank.min(5);
    Impact::new(value.min(10)).unwrap_or_else(|_| Impact::new(1).expect("1 is valid"))
}

/// Confidence never exceeds the §29 ceiling of the strongest evidence
/// tier, no matter how much evidence piles up.
#[must_use]
pub fn estimate_confidence(evidence: &[Evidence]) -> Confidence {
    let Some(strongest) = evidence.iter().map(|item| item.kind).max() else {
        return Confidence::VeryLow;
    };
    confidence_ceiling(strongest)
}

/// Category-based effort defaults (spec §30), raised when the change
/// involves `unsafe` regions (spec §54).
#[must_use]
pub fn estimate_effort(candidate: &Candidate) -> Effort {
    let base = match candidate.category {
        Category::Dependency | Category::BinarySize | Category::CompileTime => Effort::Low,
        Category::Allocation
        | Category::DataStructure
        | Category::Io
        | Category::Simd
        | Category::Cache
        | Category::Serialisation
        | Category::Parallelism
        | Category::Concurrency
        | Category::Cpu => Effort::Medium,
        Category::Algorithm
        | Category::Architecture
        | Category::Memory
        | Category::Wasm
        | Category::Gpu => Effort::High,
    };
    let touches_unsafe = candidate.evidence.iter().any(|evidence| {
        evidence
            .location
            .as_ref()
            .is_some_and(|location| location.unsafe_region)
    });
    if touches_unsafe {
        Effort::from_scale((base.scale() + 2).min(9))
    } else {
        base
    }
}

/// Risk estimation (spec §31, §54, §55): numerical code raises the
/// numerical-equivalence risk, `unsafe` regions raise the correctness
/// and unsafe-dimension risks.
#[must_use]
pub fn estimate_risk(candidate: &Candidate) -> Risk {
    let numerical = candidate.numerical_context;
    let unsafe_region = candidate.evidence.iter().any(|evidence| {
        evidence
            .location
            .as_ref()
            .is_some_and(|location| location.unsafe_region)
    });
    let concurrency_heavy = matches!(
        candidate.category,
        Category::Concurrency | Category::Parallelism
    );

    let mut overall = 3_u8;
    if numerical {
        overall += 2;
    }
    if unsafe_region {
        overall += 3;
    }
    if concurrency_heavy {
        overall += 1;
    }
    let overall =
        RiskScore::new(overall.min(10)).unwrap_or_else(|_| RiskScore::new(1).expect("1 is valid"));
    let dimension = |value: u8| RiskScore::new(value).ok();
    Risk {
        overall,
        dimensions: RiskDimensions {
            correctness: unsafe_region.then(|| dimension(7)).flatten(),
            numerical: numerical.then(|| dimension(7)).flatten(),
            concurrency: concurrency_heavy.then(|| dimension(6)).flatten(),
            unsafe_region: unsafe_region.then(|| dimension(8)).flatten(),
            portability: matches!(candidate.category, Category::Simd | Category::Gpu)
                .then(|| dimension(5))
                .flatten(),
            api: None,
        },
    }
}

/// Generality (spec §32): the hint wins when present; otherwise a
/// category default. Cross-crate categories default broader than
/// single-function ones.
#[must_use]
pub fn estimate_generality(candidate: &Candidate) -> Generality {
    if let Some(value) = candidate.generality_hint {
        return Generality::new(value.clamp(1, 10))
            .unwrap_or_else(|_| Generality::new(5).expect("5 is valid"));
    }
    let value = match candidate.category {
        Category::Dependency | Category::Architecture | Category::CompileTime => 7,
        Category::Allocation | Category::Serialisation | Category::Concurrency => 6,
        Category::Simd | Category::Parallelism | Category::Memory | Category::Cache => 5,
        Category::Algorithm | Category::DataStructure | Category::Io | Category::Cpu => 4,
        Category::BinarySize | Category::Wasm | Category::Gpu => 3,
    };
    Generality::new(value).expect("category defaults are in range")
}

/// One-line reasoning for the confidence estimate (`explain` support).
#[must_use]
pub fn confidence_reasoning(evidence: &[Evidence]) -> String {
    if evidence.is_empty() {
        return String::from("no evidence at all");
    }
    let kinds: Vec<&str> = evidence
        .iter()
        .map(|item| match item.kind {
            tpt_profiler_model::EvidenceKind::BenchmarkRegression => "benchmark regression",
            tpt_profiler_model::EvidenceKind::RuntimeProfile => "runtime profile",
            tpt_profiler_model::EvidenceKind::HardwareCounter => "hardware counter",
            tpt_profiler_model::EvidenceKind::StaticPattern => "static pattern",
            tpt_profiler_model::EvidenceKind::Heuristic => "heuristic",
        })
        .collect();
    format!(
        "{} evidence item(s): {}; ceiling per the §29 hierarchy",
        evidence.len(),
        kinds.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidate::Candidate;
    use tpt_profiler_model::SourceLocation;

    fn static_candidate() -> Candidate {
        let mut candidate = Candidate::new("repo", Category::Simd, "mdct::transform", "loop");
        candidate.evidence.push(Evidence::static_pattern(
            "element-wise arithmetic",
            SourceLocation::at_line("src/mdct.rs", 42),
        ));
        candidate
    }

    #[test]
    fn impact_tracks_evidence_strength() {
        assert_eq!(estimate_impact(1).value(), 4, "heuristic only");
        assert_eq!(estimate_impact(2).value(), 5, "static pattern");
        assert_eq!(estimate_impact(5).value(), 8, "benchmark regression");
    }

    #[test]
    fn confidence_never_exceeds_the_hierarchy_ceiling() {
        let candidate = static_candidate();
        let estimates = estimate(&candidate);
        assert_eq!(estimates.confidence, Confidence::Medium);
        assert_eq!(estimate_confidence(&[]), Confidence::VeryLow);
    }

    #[test]
    fn unsafe_evidence_raises_effort_and_risk() {
        let mut candidate = Candidate::new("repo", Category::Simd, "kernel", "loop");
        candidate.evidence.push(Evidence::static_pattern(
            "vectorisable loop",
            SourceLocation::at_line("src/k.rs", 7).with_unsafe_region(),
        ));
        let estimates = estimate(&candidate);
        assert!(estimates.effort.scale() >= Effort::Medium.scale() + 2);
        assert_eq!(
            estimates
                .risk
                .dimensions
                .unsafe_region
                .map(RiskScore::value),
            Some(8)
        );
        assert!(estimates.risk.overall.value() > 3);
    }

    #[test]
    fn numerical_code_raises_numerical_risk() {
        let mut candidate = Candidate::new("repo", Category::Simd, "kernel", "fft");
        candidate.numerical_context = true;
        let estimates = estimate(&candidate);
        assert_eq!(
            estimates.risk.dimensions.numerical.map(RiskScore::value),
            Some(7)
        );
    }

    #[test]
    fn generality_hint_wins() {
        let mut candidate = static_candidate();
        candidate.generality_hint = Some(9);
        assert_eq!(estimate(&candidate).generality.value(), 9);
    }
}
