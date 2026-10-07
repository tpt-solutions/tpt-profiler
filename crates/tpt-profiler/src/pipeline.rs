//! The orchestrator pipeline (spec §5, §62):
//!
//! ```text
//! discovery → analysis → evidence → opportunities → priority → report
//! ```
//!
//! This module is the glue the CLI calls: it owns no analysis logic
//! itself, it sequences the crates and converts between their types.
//! Runtime/bench execution (spec §8.2–8.3) stay opt-in behind the
//! config's profiling toggles; the MVP `full` run covers discovery,
//! static analysis, dependency analysis, and scoring.

use std::path::{Path, PathBuf};

use tpt_profiler_model::error::Error;
use tpt_profiler_model::hierarchy::Repository;
use tpt_profiler_model::{AnalysisOutcome, Category, Evidence, Opportunity, ProfileRun};

use crate::config::Config;
use crate::deps::{self, DependencyFinding, DependencyReport};
use crate::discovery::{discover_repository, load_metadata};
use tpt_profiler_static::{Finding, StaticEngine, StaticReport};

/// Everything one `full`-style run produced.
#[derive(Debug, Default)]
pub struct AnalysisRun {
    /// Discovered project structure (spec §7).
    pub repository: Repository,
    /// Workspace root that was analysed.
    pub workspace_root: PathBuf,
    /// Static findings as parsed (spec §9–13).
    pub static_report: StaticReport,
    /// Dependency-graph analysis (spec §18).
    pub dependency_report: DependencyReport,
    /// The ranked opportunities (spec §25, §59 order).
    pub opportunities: Vec<Opportunity>,
    /// Whether the static analysis found nothing (spec §74: a result,
    /// not a failure).
    pub no_opportunities: bool,
}

/// Runs the full static-side pipeline over the project at `root`
/// (spec §62 MVP scope).
pub fn analyse(root: &Path, config: &Config) -> Result<AnalysisRun, Error> {
    let (repository, workspace_root) = discover_repository(root)?;
    let engine = StaticEngine::with_default_analyzers();
    let static_report = engine.analyze_path(&workspace_root)?;
    let metadata = load_metadata(root)?;
    let dependency_report = deps::analyze(&metadata);

    let mut candidates = Vec::new();
    candidates.extend(findings_to_candidates(
        &repository.name,
        &static_report.findings,
    ));
    candidates.extend(dependency_candidates(&repository.name, &dependency_report));

    let repository_name = repository.name.clone();
    let mut run = AnalysisRun {
        repository,
        workspace_root,
        static_report,
        dependency_report,
        opportunities: Vec::new(),
        no_opportunities: false,
    };
    match tpt_profiler_opportunity::build_opportunities(
        &repository_name,
        &candidates,
        &tpt_profiler_opportunity::ScoringWeights {
            impact_weight: config.scoring.impact_weight,
            confidence_weight: config.scoring.confidence_weight,
            generality_weight: config.scoring.generality_weight,
            effort_weight: config.scoring.effort_weight,
            risk_weight: config.scoring.risk_weight,
        },
    ) {
        AnalysisOutcome::NoOpportunity => run.no_opportunities = true,
        AnalysisOutcome::Findings(opportunities) => run.opportunities = opportunities,
    }
    Ok(run)
}

/// Wraps a run into a [`ProfileRun`] for persistence (spec §6, §49).
#[must_use]
pub fn to_profile_run(
    run: &AnalysisRun,
    run_id: &str,
    started_at: i64,
    environment: tpt_profiler_model::Environment,
) -> ProfileRun {
    let mut profile = ProfileRun::new(run_id, started_at, environment);
    profile.opportunities = run.opportunities.clone();
    profile
}

/// Maps static findings onto engine candidates.
fn findings_to_candidates(
    repository: &str,
    findings: &[Finding],
) -> Vec<tpt_profiler_opportunity::Candidate> {
    findings
        .iter()
        .map(|finding| {
            let mut candidate = tpt_profiler_opportunity::Candidate::new(
                repository,
                finding.category,
                finding.location.file.clone(),
                finding.description.clone(),
            );
            candidate.evidence.push(finding.to_evidence());
            candidate.numerical_context = finding.numerical_context;
            candidate
        })
        .collect()
}

/// Maps dependency findings onto candidates (all structural facts about
/// the graph: `StaticPattern` evidence at `Medium` confidence).
fn dependency_candidates(
    repository: &str,
    report: &DependencyReport,
) -> Vec<tpt_profiler_opportunity::Candidate> {
    report
        .findings
        .iter()
        .map(|finding: &DependencyFinding| {
            let mut candidate = tpt_profiler_opportunity::Candidate::new(
                repository,
                Category::Dependency,
                "dependencies",
                finding.title.clone(),
            );
            candidate.evidence.push(Evidence {
                kind: tpt_profiler_model::EvidenceKind::StaticPattern,
                description: finding.description.clone(),
                location: None,
                measurement: None,
            });
            candidate
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn pipeline_runs_end_to_end_over_a_fixture() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/single-crate");
        let config = Config::default();
        let run = analyse(&root, &config).expect("pipeline runs");
        assert_eq!(run.repository.name, "single-crate");
        assert!(run.static_report.files_analyzed > 0);
        // The fixture is clean: no opportunities is a valid result (§74).
        assert!(run.no_opportunities);
        assert!(run.opportunities.is_empty());
    }

    #[test]
    fn profile_run_wraps_opportunities() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/single-crate");
        let run = analyse(&root, &Config::default()).expect("pipeline runs");
        let profile = to_profile_run(
            &run,
            "run-1",
            1_760_000_000,
            tpt_profiler_model::Environment::default(),
        );
        assert_eq!(profile.opportunities.len(), run.opportunities.len());
        assert_eq!(profile.tool_version, tpt_profiler_model::version());
    }
}
