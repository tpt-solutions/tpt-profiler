//! The opportunity engine (spec §25, §27, §51, §58, §59): candidates in,
//! ranked opportunities out.

use std::collections::BTreeMap;

use tpt_profiler_model::{
    AnalysisOutcome, Category, FindingKind, Opportunity, OpportunityId, Relationship,
    RelationshipKind,
};

use crate::candidate::Candidate;
use crate::estimate::{confidence_reasoning, estimate};
use crate::score::{ScoringTrace, ScoringWeights, compute_priority};

/// Minimum related findings before they merge into one aggregated
/// opportunity (spec §58: prevent overwhelming lists of minor findings).
pub const AGGREGATION_THRESHOLD: usize = 3;

/// Prioritisation strategy (spec §59). More strategies may follow; the
/// scoring formula underneath is not an API contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Strategy {
    /// Highest expected value first (priority, then stable ID).
    #[default]
    HighestValue,
}

/// Converts candidates into ranked opportunities (spec §25, §3.1.12).
///
/// Candidates with no evidence and rarely-executed patterns (spec §2)
/// are rejected — the outcome is "no opportunity", never an error
/// (spec §74). Related findings in the same component and category are
/// aggregated when they exceed [`AGGREGATION_THRESHOLD`] (spec §58).
#[must_use]
pub fn build_opportunities(
    repository: &str,
    candidates: &[Candidate],
    weights: &ScoringWeights,
) -> AnalysisOutcome<Vec<Opportunity>> {
    let mut opportunities = Vec::new();
    for candidate in aggregate(candidates) {
        if candidate.evidence.is_empty() {
            continue; // insufficient evidence (spec §3.1.12)
        }
        if candidate.rarely_executed() {
            continue; // §2: optimising rarely-executed code is pointless
        }
        let id = OpportunityId::generate(
            candidate.category.as_slug(),
            &candidate.component,
            &candidate.description,
        );
        let dimensions = estimate(&candidate);
        let (priority, _trace) = compute_priority(
            dimensions.impact,
            dimensions.confidence,
            dimensions.generality,
            dimensions.effort,
            &dimensions.risk,
            weights,
        );
        let kind = if candidate
            .evidence
            .iter()
            .any(|evidence| evidence.measurement.is_some())
        {
            FindingKind::Statistical
        } else {
            FindingKind::Deterministic
        };
        let interpretation = tpt_profiler_model::Interpretation {
            statement: candidate.description.clone(),
            confidence: dimensions.confidence,
        };
        let recommendation = tpt_profiler_model::Recommendation {
            action: recommended_action(candidate.category),
            rationale: format!(
                "{} — {}",
                candidate.description,
                confidence_reasoning(&candidate.evidence)
            ),
        };
        opportunities.push(Opportunity {
            id,
            repository: candidate.repository.clone(),
            component: candidate.component.clone(),
            category: candidate.category,
            description: candidate.description.clone(),
            evidence: candidate.evidence.clone(),
            impact: dimensions.impact,
            confidence: dimensions.confidence,
            effort: dimensions.effort,
            risk: dimensions.risk,
            generality: dimensions.generality,
            priority: Some(priority),
            interpretation,
            recommendation,
            state: tpt_profiler_model::LifecycleState::initial(),
            kind,
            relationships: Vec::new(),
        });
    }

    if opportunities.is_empty() {
        return AnalysisOutcome::NoOpportunity;
    }
    relate(&mut opportunities);
    sort_by_strategy(&mut opportunities, Strategy::HighestValue);
    let _ = repository;
    AnalysisOutcome::Findings(opportunities)
}

/// Aggregates related candidates (spec §58): same category + component,
/// when there are at least [`AGGREGATION_THRESHOLD`].
#[must_use]
pub fn aggregate(candidates: &[Candidate]) -> Vec<Candidate> {
    let mut groups: BTreeMap<(Category, String), Vec<Candidate>> = BTreeMap::new();
    let mut singletons = Vec::new();
    for candidate in candidates {
        let key = (candidate.category, candidate.component.clone());
        match groups.get_mut(&key) {
            Some(group) => group.push(candidate.clone()),
            None => {
                groups.insert(key, vec![candidate.clone()]);
            }
        }
    }
    for (_, mut group) in groups {
        if group.len() >= AGGREGATION_THRESHOLD {
            let mut merged = group.remove(0);
            merged.description = format!(
                "{} (+{} related findings in this component)",
                merged.description,
                group.len()
            );
            for other in group {
                merged.evidence.extend(other.evidence);
                merged.numerical_context |= other.numerical_context;
            }
            singletons.push(merged);
        } else {
            singletons.append(&mut group);
        }
    }
    singletons
}

/// Rule-based dependency edges (spec §35): SIMD opportunities in a
/// component relate to data-structure opportunities there (data layout
/// usually gates vectorisation).
fn relate(opportunities: &mut [Opportunity]) {
    let data_structure_ids: Vec<(String, OpportunityId)> = opportunities
        .iter()
        .filter(|opportunity| opportunity.category == Category::DataStructure)
        .map(|opportunity| (opportunity.component.clone(), opportunity.id.clone()))
        .collect();
    for opportunity in opportunities.iter_mut() {
        if opportunity.category != Category::Simd {
            continue;
        }
        for (component, id) in &data_structure_ids {
            if component == &opportunity.component {
                opportunity.relationships.push(Relationship {
                    target: id.clone(),
                    kind: RelationshipKind::RelatedTo,
                });
            }
        }
    }
}

/// Stable strategy ordering (spec §59, §75): priority descending, ties
/// broken by ID.
pub fn sort_by_strategy(opportunities: &mut [Opportunity], strategy: Strategy) {
    match strategy {
        Strategy::HighestValue => {
            opportunities.sort_by(|a, b| {
                let a_priority = a.priority.map_or(0, tpt_profiler_model::Priority::value);
                let b_priority = b.priority.map_or(0, tpt_profiler_model::Priority::value);
                b_priority.cmp(&a_priority).then_with(|| a.id.cmp(&b.id))
            });
        }
    }
}

/// The top-N opportunities under a strategy (spec §59: `--strategy`).
#[must_use]
pub fn top_n(
    repository: &str,
    candidates: &[Candidate],
    weights: &ScoringWeights,
    strategy: Strategy,
    n: usize,
) -> AnalysisOutcome<Vec<Opportunity>> {
    let outcome = build_opportunities(repository, candidates, weights);
    outcome.map(|mut opportunities| {
        sort_by_strategy(&mut opportunities, strategy);
        opportunities.into_iter().take(n).collect()
    })
}

/// Human-readable explanation for an opportunity (`explain <id>`, spec
/// §41, §59): why it was detected, with evidence and scoring reasoning.
#[must_use]
pub fn explain(opportunity: &Opportunity) -> String {
    let mut lines = vec![format!(
        "{} [{}] in {} — {}",
        opportunity.id, opportunity.category, opportunity.component, opportunity.description
    )];
    lines.push(format!(
        "impact {} · confidence {} · effort {} · risk {} · generality {} · priority {}",
        opportunity.impact.value(),
        opportunity.confidence,
        opportunity.effort,
        opportunity.risk.overall.value(),
        opportunity.generality,
        opportunity
            .priority
            .map_or_else(String::new, |priority| priority.to_string())
    ));
    lines.push(format!(
        "interpretation: {} ({})",
        opportunity.interpretation.statement, opportunity.interpretation.confidence
    ));
    lines.push(format!(
        "recommendation: {} ({})",
        opportunity.recommendation.action, opportunity.recommendation.rationale
    ));
    for evidence in &opportunity.evidence {
        let location = evidence
            .location
            .as_ref()
            .map_or_else(String::new, |location| format!(" at {location}"));
        lines.push(format!(
            "evidence [{:?}]{location}: {}",
            evidence.kind, evidence.description
        ));
    }
    lines.join("\n")
}

fn recommended_action(category: Category) -> String {
    match category {
        Category::Simd => {
            String::from("investigate vectorisation (auto-vectorisation or explicit SIMD)")
        }
        Category::Parallelism => String::from("evaluate Rayon-style parallel iterators"),
        Category::Allocation => {
            String::from("reuse buffers / reserve capacity / hoist allocations")
        }
        Category::DataStructure => {
            String::from("evaluate a workload-fitting data structure (e.g. set for membership)")
        }
        Category::Dependency => {
            String::from("review the dependency: reduce, replace, or internalise")
        }
        Category::Algorithm => String::from("investigate an algorithmic improvement"),
        other => format!(
            "investigate the {} opportunity",
            other.as_slug().to_lowercase()
        ),
    }
}

/// Exposes scoring traces for `explain` output.
#[must_use]
pub fn score_trace(candidate: &Candidate, weights: &ScoringWeights) -> Option<ScoringTrace> {
    if candidate.evidence.is_empty() {
        return None;
    }
    let dimensions = estimate(candidate);
    Some(
        compute_priority(
            dimensions.impact,
            dimensions.confidence,
            dimensions.generality,
            dimensions.effort,
            &dimensions.risk,
            weights,
        )
        .1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidate::Candidate;
    use tpt_profiler_model::{Evidence, SourceLocation, Unit};

    fn simd_candidate(component: &str, description: &str) -> Candidate {
        let mut candidate = Candidate::new("repo", Category::Simd, component, description);
        candidate.evidence.push(Evidence::static_pattern(
            description,
            SourceLocation::at_line("src/a.rs", 10),
        ));
        candidate
    }

    fn weights() -> ScoringWeights {
        ScoringWeights::default()
    }

    #[test]
    fn empty_evidence_is_rejected_not_an_error() {
        let candidate = Candidate::new("repo", Category::Simd, "x", "no evidence at all");
        let outcome = build_opportunities("repo", &[candidate], &weights());
        assert!(outcome.is_no_opportunity(), "spec §3.1.12");
    }

    #[test]
    fn rarely_executed_code_is_suppressed() {
        let mut candidate = simd_candidate("cold", "loop");
        candidate.evidence.push(Evidence {
            kind: tpt_profiler_model::EvidenceKind::RuntimeProfile,
            description: String::from("cold"),
            location: None,
            measurement: Some(tpt_profiler_model::Measurement::runtime(
                "profile_coverage",
                0.001,
                Unit::Ratio,
                tpt_profiler_model::MeasurementSource::Perf,
                0,
            )),
        });
        let outcome = build_opportunities("repo", &[candidate], &weights());
        assert!(outcome.is_no_opportunity(), "§2: cold code is not worth it");
    }

    #[test]
    fn related_findings_aggregate_past_the_threshold() {
        let candidates: Vec<Candidate> = (0..4)
            .map(|index| simd_candidate("mdct", &format!("finding {index}")))
            .collect();
        let outcome = build_opportunities("repo", &candidates, &weights());
        let findings = outcome.findings().cloned().expect("findings exist");
        assert_eq!(findings.len(), 1, "four findings aggregate into one");
        assert!(findings[0].description.contains("+3 related"));
        assert_eq!(findings[0].evidence.len(), 4, "evidence merged");
    }

    #[test]
    fn below_threshold_candidates_stay_individual() {
        let candidates: Vec<Candidate> = (0..2)
            .map(|index| simd_candidate("mdct", &format!("finding {index}")))
            .collect();
        let findings = build_opportunities("repo", &candidates, &weights())
            .findings()
            .cloned()
            .expect("findings exist");
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn simd_relates_to_data_structure_in_same_component() {
        let simd = simd_candidate("mdct", "loop");
        let mut data_structure =
            Candidate::new("repo", Category::DataStructure, "mdct", "vec lookup");
        data_structure.evidence.push(Evidence::static_pattern(
            "lookup",
            SourceLocation::at_line("src/a.rs", 3),
        ));
        let findings = build_opportunities("repo", &[simd, data_structure], &weights())
            .findings()
            .cloned()
            .expect("findings exist");
        let simd_opportunity = findings
            .iter()
            .find(|opportunity| opportunity.category == Category::Simd)
            .expect("simd present");
        assert_eq!(simd_opportunity.relationships.len(), 1);
        assert_eq!(
            simd_opportunity.relationships[0].kind,
            RelationshipKind::RelatedTo
        );
    }

    #[test]
    fn top_n_returns_ranked_prefix() {
        let strong = simd_candidate("a", "strong candidate");
        let weak = Candidate::new("repo", Category::Gpu, "b", "gpu guess");
        // GPU without strong evidence: heuristic-only stays but scores low.
        let mut weak = weak;
        weak.evidence.push(Evidence {
            kind: tpt_profiler_model::EvidenceKind::Heuristic,
            description: String::from("guess"),
            location: None,
            measurement: None,
        });
        let top = top_n(
            "repo",
            &[weak, strong],
            &weights(),
            Strategy::HighestValue,
            1,
        )
        .findings()
        .cloned()
        .expect("one result");
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].category, Category::Simd, "strong ranks first");
    }

    #[test]
    fn ordering_is_stable_under_ties() {
        let first = simd_candidate("x", "tie candidate one");
        let second = simd_candidate("x", "tie candidate two");
        let mut opportunities = build_opportunities("repo", &[first, second], &weights())
            .findings()
            .cloned()
            .expect("findings exist");
        let before = opportunities.clone();
        sort_by_strategy(&mut opportunities, Strategy::HighestValue);
        // Ties break by ID, so repeated sorting is a fixed point (§75).
        sort_by_strategy(&mut opportunities, Strategy::HighestValue);
        assert_eq!(opportunities, before);
    }

    #[test]
    fn explain_contains_evidence_and_scores() {
        let findings = build_opportunities("repo", &[simd_candidate("mdct", "loop")], &weights())
            .findings()
            .cloned()
            .expect("findings exist");
        let text = explain(&findings[0]);
        assert!(text.contains("evidence [StaticPattern]"), "{text}");
        assert!(text.contains("priority"), "{text}");
        assert!(text.contains("recommendation"), "{text}");
    }
}
