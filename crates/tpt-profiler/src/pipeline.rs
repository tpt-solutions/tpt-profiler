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

use crate::baseline::STATE_DIR;
use crate::config::{Config, ProfilingConfig};
use crate::deps::{self, DependencyFinding, DependencyReport};
use crate::discovery::{load_metadata, repository_from_metadata};
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

/// Runs the full static-side pipeline over the project at `root` with
/// the built-in analyzers (spec §62 MVP scope).
pub fn analyse(root: &Path, config: &Config) -> Result<AnalysisRun, Error> {
    analyse_with(root, config, &StaticEngine::with_default_analyzers())
}

/// Like [`analyse`], but with a caller-supplied static engine, so custom
/// analyzers registered via [`StaticEngine::register`] take part in the
/// run (spec §67 analyzer seam).
pub fn analyse_with(
    root: &Path,
    config: &Config,
    engine: &StaticEngine,
) -> Result<AnalysisRun, Error> {
    let metadata = load_metadata(root)?;
    let (repository, workspace_root) = repository_from_metadata(&metadata);
    let cache_path = workspace_root
        .join(STATE_DIR)
        .join("cache")
        .join(tpt_profiler_static::cache::CACHE_FILE);
    let static_report = engine.analyze_path_cached(&workspace_root, &cache_path)?;
    let dependency_report = deps::analyze(&metadata);
    Ok(assemble(
        config,
        repository,
        workspace_root,
        static_report,
        dependency_report,
    ))
}

/// Static analysis only (spec §70): no `cargo metadata`, so no process
/// spawn, no registry access and no dependency findings. The repository is
/// named after its directory and has no crate detail. Used by ecosystem
/// scans, where hundreds of repositories must run quickly and quietly.
///
/// # Errors
/// Fails when the source tree cannot be walked or a file cannot be read.
pub fn analyse_static(root: &Path, config: &Config) -> Result<AnalysisRun, Error> {
    let name = root.file_name().map_or_else(
        || root.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let repository = Repository {
        name,
        projects: Vec::new(),
    };
    let engine = StaticEngine::with_default_analyzers();
    let static_report = engine.analyze_path(root)?;
    Ok(assemble(
        config,
        repository,
        root.to_path_buf(),
        static_report,
        DependencyReport::default(),
    ))
}

/// Turns findings and dependency results into ranked opportunities.
fn assemble(
    config: &Config,
    repository: Repository,
    workspace_root: PathBuf,
    static_report: StaticReport,
    dependency_report: DependencyReport,
) -> AnalysisRun {
    let mut candidates = Vec::new();
    candidates.extend(findings_to_candidates(
        &repository.name,
        &static_report.findings,
        config.profiling,
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
    run
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
    profile.opportunities.clone_from(&run.opportunities);
    profile
}

/// Maps static findings onto engine candidates, dropping the categories
/// whose `[profiling]` toggle is off (spec §40). Categories without a
/// toggle are always kept.
fn findings_to_candidates(
    repository: &str,
    findings: &[Finding],
    profiling: ProfilingConfig,
) -> Vec<tpt_profiler_opportunity::Candidate> {
    findings
        .iter()
        .filter(|finding| match finding.category {
            Category::Simd => profiling.simd,
            Category::Parallelism => profiling.parallelism,
            _ => true,
        })
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
