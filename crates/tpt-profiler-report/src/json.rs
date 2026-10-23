//! JSON output (spec §42, §69): every artefact is wrapped in the
//! versioned [`SchemaEnvelope`] so downstream TPT tools can pin what
//! they consume.

use serde::Serialize;
use tpt_profiler_model::{Opportunity, ProfileRun, SCHEMA_VERSION, SchemaEnvelope};

/// Encodes a profile run as pretty-printed JSON under the schema
/// envelope (spec §69): `{"schema_version": 1, "generated_by": …,
/// "data": …}`.
///
/// # Errors
/// Fails when the run cannot be serialised (spec §74:
/// `SerializationError`).
pub fn render_json(generated_by: &str, run: &ProfileRun) -> Result<String, serde_json::Error> {
    let envelope = SchemaEnvelope::new(generated_by, run);
    serde_json::to_string_pretty(&envelope)
}

/// Convenience: just the schema version identifier for headers.
#[must_use]
pub const fn schema_version() -> u32 {
    SCHEMA_VERSION
}

/// A machine-readable summary envelope for opportunity-only output
/// (spec §69 example shape).
#[derive(Debug, Serialize)]
pub struct OpportunitySummary<'a> {
    /// Repository the opportunities were found in.
    pub repository: &'a str,
    /// Opportunity entries in rank order (already sorted by the engine).
    pub opportunities: Vec<OpportunitySummaryEntry<'a>>,
}

/// One entry in the opportunity summary (spec §69 shape: numeric scales,
/// stable id, category slug).
#[derive(Debug, Serialize)]
pub struct OpportunitySummaryEntry<'a> {
    /// Stable opportunity ID (e.g. `SIMD-04217`).
    pub id: &'a str,
    /// Category slug (e.g. `SIMD`).
    pub category: &'a str,
    /// Component path.
    pub component: &'a str,
    /// Impact 1–10.
    pub impact: u8,
    /// Confidence on the numeric scale (2/4/6/8/10).
    pub confidence: u8,
    /// Effort on the numeric scale (1/3/5/7/9).
    pub effort: u8,
    /// Overall risk 1–10.
    pub risk: u8,
    /// Generality 1–10.
    pub generality: u8,
    /// Priority 0–100, when scored.
    pub priority: Option<u8>,
}

/// Builds the summary envelope for a run's opportunities.
#[must_use]
pub fn opportunity_summary(run: &ProfileRun) -> OpportunitySummary<'_> {
    summarise(&run.id, &run.opportunities)
}

/// Builds the summary for an explicit repository name and opportunity list.
#[must_use]
pub fn summarise<'a>(
    repository: &'a str,
    opportunities: &'a [Opportunity],
) -> OpportunitySummary<'a> {
    OpportunitySummary {
        repository,
        opportunities: opportunities
            .iter()
            .map(|opportunity| OpportunitySummaryEntry {
                id: opportunity.id.as_str(),
                category: opportunity.category.as_slug(),
                component: &opportunity.component,
                impact: opportunity.impact.value(),
                confidence: opportunity.confidence.scale(),
                effort: opportunity.effort.scale(),
                risk: opportunity.risk.overall.value(),
                generality: opportunity.generality.value(),
                priority: opportunity
                    .priority
                    .map(tpt_profiler_model::Priority::value),
            })
            .collect(),
    }
}

/// Encodes ranked opportunities as pretty-printed JSON under the schema
/// envelope (spec §42, §69).
///
/// # Errors
/// Fails when the summary cannot be serialised.
pub fn render_opportunities_json(
    generated_by: &str,
    repository: &str,
    opportunities: &[Opportunity],
) -> Result<String, serde_json::Error> {
    let envelope = SchemaEnvelope::new(generated_by, summarise(repository, opportunities));
    serde_json::to_string_pretty(&envelope)
}

/// Encodes the opportunity summary as JSON.
///
/// # Errors
/// Fails when the summary cannot be serialised.
pub fn render_summary_json(summary: &OpportunitySummary<'_>) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_profiler_model::{Category, Environment, Opportunity, OpportunityId};

    fn run_with_one() -> ProfileRun {
        let mut run = ProfileRun::new("run-1", 1_760_000_000, Environment::default());
        run.opportunities.push(Opportunity {
            id: OpportunityId::generate("SIMD", "mdct::transform", "vectorisable loop"),
            repository: String::from("tpt-cadence"),
            component: String::from("mdct::transform"),
            category: Category::Simd,
            description: String::from("vectorisable loop"),
            evidence: Vec::new(),
            impact: tpt_profiler_model::Impact::new(9).unwrap(),
            confidence: tpt_profiler_model::Confidence::High,
            effort: tpt_profiler_model::Effort::Medium,
            risk: tpt_profiler_model::Risk {
                overall: tpt_profiler_model::RiskScore::new(3).unwrap(),
                dimensions: tpt_profiler_model::RiskDimensions::default(),
            },
            generality: tpt_profiler_model::Generality::new(8).unwrap(),
            priority: tpt_profiler_model::Priority::new(85).ok(),
            interpretation: tpt_profiler_model::Interpretation {
                statement: String::from("hot kernel"),
                confidence: tpt_profiler_model::Confidence::High,
            },
            recommendation: tpt_profiler_model::Recommendation {
                action: String::from("vectorise"),
                rationale: String::from("independent arithmetic"),
            },
            state: tpt_profiler_model::LifecycleState::Open,
            kind: tpt_profiler_model::FindingKind::Deterministic,
            relationships: Vec::new(),
        });
        run
    }

    #[test]
    fn run_json_carries_schema_version_envelope() {
        let json = render_json("tpt-profiler test", &run_with_one()).expect("encode");
        assert!(json.contains("\"schema_version\": 1"), "{json}");
        assert!(json.contains("\"generated_by\": \"tpt-profiler test\""));
        assert!(json.contains("SIMD-"));
    }

    #[test]
    fn run_json_round_trips_through_the_model() {
        let run = run_with_one();
        let json = render_json("test", &run).expect("encode");
        let decoded: SchemaEnvelope<ProfileRun> = serde_json::from_str(&json).expect("decode");
        assert_eq!(decoded.validate().expect("valid"), SCHEMA_VERSION);
        assert_eq!(decoded.data, run);
    }

    #[test]
    fn summary_matches_the_spec_69_shape() {
        let run = run_with_one();
        let summary = opportunity_summary(&run);
        let json = render_summary_json(&summary).expect("encode");
        assert!(json.contains("\"id\": \"SIMD-"), "{json}");
        assert!(json.contains("\"category\": \"SIMD\""));
        assert!(json.contains("\"impact\": 9"));
        assert!(json.contains("\"priority\": 85"));
    }
}
