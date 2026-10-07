//! Markdown output (spec §42): a ranked table plus per-opportunity
//! sections, always distinguishing measured facts from static findings
//! (spec §29).

use std::fmt::Write as _;

use tpt_profiler_model::{FindingKind, Opportunity};

/// Renders opportunities as a Markdown report.
#[must_use]
pub fn render_markdown(repository: &str, opportunities: &[Opportunity]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# tpt-profiler report — {repository}");
    let _ = writeln!(out);
    if opportunities.is_empty() {
        let _ = writeln!(out, "No opportunities detected.");
        return out;
    }
    let _ = writeln!(
        out,
        "| # | ID | Category | Component | Impact | Confidence | Effort | Priority | Basis |"
    );
    let _ = writeln!(
        out,
        "|---|----|----------|-----------|--------|------------|--------|----------|-------|"
    );
    for (index, opportunity) in opportunities.iter().enumerate() {
        let _ = writeln!(
            out,
            "| {} | {} | {} | `{}` | {}/10 | {} | {} | {} | {} |",
            index + 1,
            opportunity.id,
            opportunity.category,
            opportunity.component,
            opportunity.impact.value(),
            opportunity.confidence,
            opportunity.effort,
            opportunity
                .priority
                .map_or_else(|| String::from("n/a"), |priority| priority.to_string()),
            basis(opportunity.kind),
        );
    }
    let _ = writeln!(out);
    for opportunity in opportunities {
        let _ = writeln!(out, "## {} — {}", opportunity.id, opportunity.description);
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "- **Interpretation:** {} ({})",
            opportunity.interpretation.statement, opportunity.interpretation.confidence
        );
        let _ = writeln!(
            out,
            "- **Recommendation:** {} — {}",
            opportunity.recommendation.action, opportunity.recommendation.rationale
        );
        let _ = writeln!(
            out,
            "- **Risk:** overall {}/10{}",
            opportunity.risk.overall.value(),
            dimension_notes(opportunity)
        );
        if !opportunity.evidence.is_empty() {
            let _ = writeln!(out, "- **Evidence:**");
            for evidence in &opportunity.evidence {
                let location = evidence
                    .location
                    .as_ref()
                    .map_or_else(String::new, |location| format!(" (`{location}`)"));
                let _ = writeln!(
                    out,
                    "  - {:?}{}: {}",
                    evidence.kind, location, evidence.description
                );
            }
        }
        let _ = writeln!(out);
    }
    out
}

fn basis(kind: FindingKind) -> &'static str {
    match kind {
        FindingKind::Deterministic => "static (deterministic)",
        FindingKind::Statistical => "measured (statistical)",
    }
}

fn dimension_notes(opportunity: &Opportunity) -> String {
    let mut notes = String::new();
    let dimensions = &opportunity.risk.dimensions;
    for (label, value) in [
        ("numerical", dimensions.numerical),
        ("unsafe", dimensions.unsafe_region),
        ("concurrency", dimensions.concurrency),
    ] {
        if let Some(value) = value {
            let _ = write!(notes, ", {label} {}/10", value.value());
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::String;
    use std::vec;
    use tpt_profiler_model::{
        Category, Confidence, Effort, Evidence, Impact, Interpretation, LifecycleState,
        Opportunity, OpportunityId, Priority, Recommendation, Risk, RiskDimensions, RiskScore,
        SourceLocation,
    };

    fn sample() -> Opportunity {
        Opportunity {
            id: OpportunityId::generate("SIMD", "mdct::transform", "vectorisable loop"),
            repository: String::from("tpt-cadence"),
            component: String::from("mdct::transform"),
            category: Category::Simd,
            description: String::from("independent scalar arithmetic over contiguous input"),
            evidence: vec![Evidence::static_pattern(
                "element-wise arithmetic",
                SourceLocation::at_line("src/mdct.rs", 42),
            )],
            impact: Impact::new(9).unwrap(),
            confidence: Confidence::High,
            effort: Effort::Medium,
            risk: Risk {
                overall: RiskScore::new(3).unwrap(),
                dimensions: RiskDimensions {
                    numerical: RiskScore::new(7).ok(),
                    ..RiskDimensions::default()
                },
            },
            generality: tpt_profiler_model::Generality::new(8).unwrap(),
            priority: Priority::new(85).ok(),
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
            relationships: Vec::new(),
        }
    }

    #[test]
    fn markdown_has_ranked_table_and_sections() {
        let markdown = render_markdown("tpt-cadence", &[sample()]);
        assert!(markdown.contains("# tpt-profiler report — tpt-cadence"));
        assert!(markdown.contains("| 1 | SIMD-"));
        assert!(markdown.contains("static (deterministic)"));
        assert!(markdown.contains("## SIMD-"));
        assert!(markdown.contains("**Evidence:**"));
        assert!(markdown.contains("`src/mdct.rs:42`"));
        assert!(
            markdown.contains("numerical 7/10"),
            "risk dimensions surface: {markdown}"
        );
    }

    #[test]
    fn measured_facts_are_labelled_measured() {
        let mut opportunity = sample();
        opportunity.kind = FindingKind::Statistical;
        let markdown = render_markdown("repo", &[opportunity]);
        assert!(markdown.contains("measured (statistical)"));
    }

    #[test]
    fn empty_report_states_the_result() {
        let markdown = render_markdown("repo", &[]);
        assert!(markdown.contains("No opportunities detected."));
    }
}
