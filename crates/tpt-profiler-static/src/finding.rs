//! Findings: the standardised output unit of every analyzer (spec §9, §67).
//!
//! A [`Finding`] is static-analysis *evidence*, never a prescription: it
//! records what pattern was seen, where, and with what confidence. The
//! opportunity engine (spec §25) converts findings into opportunities;
//! grouping happens there (spec §58) using [`Finding::group_key`].

use quote::ToTokens;
use tpt_profiler_model::{Category, Confidence, Evidence, EvidenceKind, SourceLocation};

/// One static-analysis detection (spec §9: evidence and confidence, no
/// blind flagging).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Optimisation category (spec §26).
    pub category: Category,
    /// Registered analyzer that produced this finding (spec §67).
    pub detector: &'static str,
    /// Human-readable description with the actionable hint.
    pub description: String,
    /// Where the pattern was found.
    pub location: SourceLocation,
    /// Detector's confidence in this finding (spec §29). The evidence
    /// hierarchy caps static findings at `Medium`.
    pub confidence: Confidence,
    /// Whether the finding sits in floating-point code (spec §55): set
    /// by the engine, and a risk signal — faster is not equivalent.
    pub numerical_context: bool,
}

impl Finding {
    /// Constructs a finding; `numerical_context` starts `false` and is
    /// filled in by the engine.
    #[must_use]
    pub fn new(
        detector: &'static str,
        category: Category,
        description: impl Into<String>,
        location: SourceLocation,
        confidence: Confidence,
    ) -> Self {
        Self {
            category,
            detector,
            description: description.into(),
            location,
            confidence,
            numerical_context: false,
        }
    }

    /// Converts the finding into model evidence. Findings at
    /// [`Confidence::Medium`] or above map to
    /// [`EvidenceKind::StaticPattern`]; weaker ones to
    /// [`EvidenceKind::Heuristic`] (spec §29).
    #[must_use]
    pub fn to_evidence(&self) -> Evidence {
        let kind = if self.confidence >= Confidence::Medium {
            EvidenceKind::StaticPattern
        } else {
            EvidenceKind::Heuristic
        };
        Evidence {
            kind,
            description: self.description.clone(),
            location: Some(self.location.clone()),
            measurement: None,
        }
    }

    /// Key for aggregating related findings into one opportunity
    /// (spec §58): same category in the same file.
    #[must_use]
    pub fn group_key(&self) -> (Category, String) {
        (self.category, self.location.file.clone())
    }
}

/// Renders any syn node back to (whitespace-normalised) source text.
pub(crate) fn render<T: ToTokens>(node: &T) -> String {
    node.to_token_stream().to_string().replace(' ', "")
}

/// Sorts findings into deterministic order (spec §75): file, then line,
/// then detector, then description.
pub(crate) fn sort_findings(findings: &mut [Finding]) {
    findings.sort_by(|a, b| {
        (
            &a.location.file,
            a.location.line_start,
            a.detector,
            &a.description,
        )
            .cmp(&(
                &b.location.file,
                b.location.line_start,
                b.detector,
                &b.description,
            ))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_kind_follows_confidence() {
        let strong = Finding::new(
            "test",
            Category::Allocation,
            "allocation in loop",
            SourceLocation::at_line("src/a.rs", 3),
            Confidence::Medium,
        );
        let weak = Finding::new(
            "test",
            Category::Parallelism,
            "parallel candidate",
            SourceLocation::at_line("src/a.rs", 9),
            Confidence::Low,
        );
        assert_eq!(strong.to_evidence().kind, EvidenceKind::StaticPattern);
        assert_eq!(weak.to_evidence().kind, EvidenceKind::Heuristic);
    }

    #[test]
    fn group_key_groups_by_category_and_file() {
        let a = Finding::new(
            "test",
            Category::Allocation,
            "one",
            SourceLocation::at_line("src/a.rs", 3),
            Confidence::Low,
        );
        let b = Finding::new(
            "test",
            Category::Allocation,
            "two",
            SourceLocation::at_line("src/a.rs", 9),
            Confidence::Low,
        );
        assert_eq!(a.group_key(), b.group_key());
    }
}
