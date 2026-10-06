//! The Opportunity: the fundamental unit of `tpt-profiler` (spec §25).
//!
//! An opportunity bundles the four things the spec insists stay separate
//! (§51): measurements ([`Evidence`], observed facts), an
//! [`Interpretation`] (what that means), a [`Recommendation`] (what to
//! do), and the scoring that ranks it (spec §27–§32).

use alloc::string::String;
use alloc::vec::Vec;

use crate::category::Category;
use crate::evidence::Evidence;
use crate::id::OpportunityId;
use crate::lifecycle::LifecycleState;
use crate::measurement::FindingKind;
use crate::scales::{Confidence, Effort, Generality, Impact, Priority, Risk};

/// A relationship edge between two opportunities (spec §35), e.g. a SIMD
/// optimisation that requires a data-layout change first.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum RelationshipKind {
    /// This opportunity requires the other to land first.
    Requires,
    /// This opportunity blocks the other.
    Blocks,
    /// This opportunity supersedes the other.
    Supersedes,
    /// Loosely related, no ordering.
    RelatedTo,
}

/// A directed relationship to another opportunity (spec §35).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Relationship {
    /// The other end of the edge.
    pub target: OpportunityId,
    /// What kind of edge this is.
    pub kind: RelationshipKind,
}

/// What the evidence *means* — an interpretation, not a measurement
/// (spec §51).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Interpretation {
    /// One-sentence reading of the evidence (e.g. "potential hotspot").
    pub statement: String,
    /// How strongly the evidence supports this interpretation.
    pub confidence: Confidence,
}

/// What the engineer should consider doing — a recommendation, never an
/// automatic rewrite (spec §51, §53).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Recommendation {
    /// The suggested action (e.g. "introduce reusable decode buffer").
    pub action: String,
    /// Why this action, referencing the interpretation.
    pub rationale: String,
}

/// A ranked, explained, actionable optimisation opportunity (spec §25).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Opportunity {
    /// Stable, deterministic ID (spec §25, §75).
    pub id: OpportunityId,
    /// Repository the opportunity was found in.
    pub repository: String,
    /// Component path, e.g. `mdct::transform`.
    pub component: String,
    /// Optimisation category (spec §26).
    pub category: Category,
    /// Short description of the opportunity.
    pub description: String,
    /// Supporting evidence, ranked per spec §29.
    pub evidence: Vec<Evidence>,
    /// Expected impact, 1–10 (spec §28).
    pub impact: Impact,
    /// Confidence in the recommendation (spec §29).
    pub confidence: Confidence,
    /// Estimated effort (spec §30).
    pub effort: Effort,
    /// Risk, including numerical/unsafe dimensions (spec §31, §54, §55).
    pub risk: Risk,
    /// How broadly the fix applies (spec §32).
    pub generality: Generality,
    /// Priority assigned by the scoring engine (spec §27); `None` until
    /// scored.
    pub priority: Option<Priority>,
    /// Interpretation of the evidence (spec §51).
    pub interpretation: Interpretation,
    /// Recommended action (spec §51).
    pub recommendation: Recommendation,
    /// Lifecycle position (spec §38).
    pub state: LifecycleState,
    /// Deterministic (static) vs statistical (runtime-derived) finding
    /// (spec §75).
    pub kind: FindingKind,
    /// Edges to other opportunities (spec §35).
    pub relationships: Vec<Relationship>,
}

impl Opportunity {
    /// The category slug, for use with [`OpportunityId::generate`].
    #[must_use]
    pub fn slug(&self) -> &'static str {
        self.category.as_slug()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scales::{RiskDimensions, RiskScore};
    use alloc::vec;

    #[test]
    fn opportunity_holds_spec_25_fields() {
        let id = OpportunityId::generate("SIMD", "mdct::transform", "vectorisable loop");
        let opportunity = Opportunity {
            id,
            repository: String::from("tpt-cadence"),
            component: String::from("mdct::transform"),
            category: Category::Simd,
            description: String::from("independent scalar arithmetic"),
            evidence: vec![],
            impact: Impact::new(9).unwrap(),
            confidence: Confidence::VeryHigh,
            effort: Effort::Medium,
            risk: Risk {
                overall: RiskScore::new(3).unwrap(),
                dimensions: RiskDimensions::default(),
            },
            generality: Generality::new(8).unwrap(),
            priority: Priority::new(86).ok(),
            interpretation: Interpretation {
                statement: String::from("hot numerical kernel"),
                confidence: Confidence::High,
            },
            recommendation: Recommendation {
                action: String::from("investigate vectorisation"),
                rationale: String::from("independent fixed-width arithmetic"),
            },
            state: LifecycleState::Open,
            kind: FindingKind::Deterministic,
            relationships: vec![],
        };
        assert_eq!(opportunity.slug(), "SIMD");
        assert!(opportunity.id.as_str().starts_with("SIMD-"));
        assert_eq!(opportunity.priority.unwrap().value(), 86);
    }
}
