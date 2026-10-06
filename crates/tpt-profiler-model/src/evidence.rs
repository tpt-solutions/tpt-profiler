//! Evidence: typed support for findings, ranked per the spec §29
//! hierarchy.

use alloc::string::String;

use crate::location::SourceLocation;
use crate::measurement::Measurement;
use crate::scales::Confidence;

/// Kind of evidence, ordered by strength per the spec §29 hierarchy:
///
/// ```text
/// observed benchmark regression  (strongest)
///   ↓ runtime profile evidence
///   ↓ hardware counter evidence
///   ↓ static analysis + known pattern
///   ↓ heuristic                    (weakest)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum EvidenceKind {
    /// An observed benchmark regression (strongest).
    BenchmarkRegression,
    /// Runtime profiler output attributing cost to this code.
    RuntimeProfile,
    /// Hardware counters (cycles, cache misses, branches; spec §8.5).
    HardwareCounter,
    /// Static analysis matched a known pattern (spec §9).
    StaticPattern,
    /// A heuristic guess without direct observation (weakest).
    Heuristic,
}

impl EvidenceKind {
    /// Strength rank, 5 (strongest) down to 1 (weakest). Higher wins.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::BenchmarkRegression => 5,
            Self::RuntimeProfile => 4,
            Self::HardwareCounter => 3,
            Self::StaticPattern => 2,
            Self::Heuristic => 1,
        }
    }
}

/// One piece of evidence supporting (or contradicting) a finding.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Evidence {
    /// Which tier of the §29 hierarchy this evidence comes from.
    pub kind: EvidenceKind,
    /// What was observed, in one or two sentences.
    pub description: String,
    /// Where in the source this applies, when location-bound.
    pub location: Option<SourceLocation>,
    /// A backing measurement, when this evidence is directly observed
    /// (spec §51: kept separate from interpretation).
    pub measurement: Option<Measurement>,
}

impl Evidence {
    /// Static-pattern evidence without a measurement.
    #[must_use]
    pub fn static_pattern(description: impl Into<String>, location: SourceLocation) -> Self {
        Self {
            kind: EvidenceKind::StaticPattern,
            description: description.into(),
            location: Some(location),
            measurement: None,
        }
    }

    /// Evidence backed by an observed measurement; confidence floors at
    /// the measurement's own confidence (spec §29: measured beats
    /// predicted).
    #[must_use]
    pub fn measured(
        kind: EvidenceKind,
        description: impl Into<String>,
        measurement: Measurement,
    ) -> Self {
        Self {
            kind,
            description: description.into(),
            location: None,
            measurement: Some(measurement),
        }
    }
}

/// Returns the strongest evidence rank in `evidence` (0 when empty).
#[must_use]
pub fn strongest_rank(evidence: &[Evidence]) -> u8 {
    evidence
        .iter()
        .map(|evidence_item| evidence_item.kind.rank())
        .max()
        .unwrap_or(0)
}

/// Suggests a confidence level for evidence of the given strongest kind
/// (spec §29: the hierarchy bounds what the opportunity engine may
/// claim). Static patterns can at most justify `Medium`; heuristics at
/// most `Low`.
#[must_use]
pub const fn confidence_ceiling(kind: EvidenceKind) -> Confidence {
    match kind {
        EvidenceKind::BenchmarkRegression => Confidence::VeryHigh,
        EvidenceKind::RuntimeProfile | EvidenceKind::HardwareCounter => Confidence::High,
        EvidenceKind::StaticPattern => Confidence::Medium,
        EvidenceKind::Heuristic => Confidence::Low,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measurement::{MeasurementSource, Unit};

    #[test]
    fn hierarchy_order_matches_spec_29() {
        assert!(EvidenceKind::BenchmarkRegression.rank() > EvidenceKind::RuntimeProfile.rank());
        assert!(EvidenceKind::RuntimeProfile.rank() > EvidenceKind::HardwareCounter.rank());
        assert!(EvidenceKind::HardwareCounter.rank() > EvidenceKind::StaticPattern.rank());
        assert!(EvidenceKind::StaticPattern.rank() > EvidenceKind::Heuristic.rank());
    }

    #[test]
    fn strongest_rank_picks_the_best_tier() {
        let evidence = alloc::vec![
            Evidence::static_pattern("clone in loop", SourceLocation::at_line("src/a.rs", 3)),
            Evidence::measured(
                EvidenceKind::RuntimeProfile,
                "41% CPU in decode_frame",
                Measurement::runtime("cpu_time", 1.4, Unit::Seconds, MeasurementSource::Perf, 0,),
            ),
        ];
        assert_eq!(
            strongest_rank(&evidence),
            EvidenceKind::RuntimeProfile.rank()
        );
        assert_eq!(strongest_rank(&[]), 0);
    }

    #[test]
    fn static_evidence_cannot_claim_high_confidence() {
        assert_eq!(
            confidence_ceiling(EvidenceKind::StaticPattern),
            Confidence::Medium
        );
        assert_eq!(confidence_ceiling(EvidenceKind::Heuristic), Confidence::Low);
        assert_eq!(
            confidence_ceiling(EvidenceKind::BenchmarkRegression),
            Confidence::VeryHigh
        );
    }
}
