//! Runtime measurement collection shared by `runtime`, `baseline`,
//! `compare` and `ci` (spec §8.2, §23, §36, §39).
//!
//! Workloads run here, once, so every consumer sees the same metric names
//! and the same statistics. Without this, a baseline or a CI comparison
//! would contain only static findings and could never judge a runtime
//! change.

use std::path::Path;
use std::time::Duration;

use tpt_profiler_model::{
    Environment, Measurement, MeasurementSource, UnixSeconds, Workload, hierarchy::CrateInfo,
};
use tpt_profiler_runtime::{RunnableWorkload, measure_workload, to_measurements};

use crate::config::Config;

/// Upper bound for one `cargo bench` invocation (spec §8.3).
const BENCH_TIMEOUT: Duration = Duration::from_secs(1800);

/// Result of running the workspace's benchmarks (spec §8.3).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BenchOutcome {
    /// No bench targets were discovered, so nothing ran (a result, not a
    /// failure: spec §74).
    pub no_benchmarks: bool,
    /// Criterion means as measurements. Empty when benches ran but Criterion
    /// produced no data.
    pub measurements: Vec<Measurement>,
}

/// Runs the workspace's benchmarks when `[benchmarks] enabled` (spec §8.3).
///
/// Returns `None` when benchmarks are disabled in configuration, so callers
/// can tell "off" apart from "nothing to run".
///
/// # Errors
/// Fails when the workspace cannot be discovered, `cargo` is missing, or
/// `cargo bench` fails.
pub fn collect_benches(
    root: &Path,
    config: &Config,
    now: UnixSeconds,
) -> Result<Option<BenchOutcome>, tpt_profiler_model::Error> {
    if !config.benchmarks.enabled {
        return Ok(None);
    }
    let (repository, workspace_root) = crate::discovery::discover_repository(root)?;
    let crates: Vec<CrateInfo> = repository
        .projects
        .iter()
        .flat_map(|project| project.crates.clone())
        .collect();
    let report = tpt_profiler_bench::run_benches(&workspace_root, &crates, BENCH_TIMEOUT)?;
    if report.no_benchmarks {
        return Ok(Some(BenchOutcome {
            no_benchmarks: true,
            measurements: Vec::new(),
        }));
    }
    let measurements =
        tpt_profiler_bench::to_measurements(&report.estimates, &MeasurementSource::Criterion, now);
    Ok(Some(BenchOutcome {
        no_benchmarks: false,
        measurements,
    }))
}

/// Result of running one configured workload (spec §23).
#[derive(Debug, Clone, PartialEq)]
pub struct WorkloadOutcome {
    /// Workload name from `[[workloads]]`.
    pub name: String,
    /// Measurements derived from the timed samples.
    pub measurements: Vec<Measurement>,
    /// Number of samples the statistics were computed from.
    pub samples: usize,
    /// Runs that failed or timed out and were excluded.
    pub failed_runs: u32,
}

/// Runs every configured workload when `[profiling] runtime` is enabled
/// (spec §8.2). Returns an empty list when runtime profiling is off or no
/// workloads are configured.
///
/// Workload commands are resolved relative to `root`.
///
/// # Errors
/// Fails when a workload cannot be spawned at all (for example, the
/// command is missing). A workload that runs but fails is reported in
/// [`WorkloadOutcome::failed_runs`] instead.
pub fn collect_runtime(
    root: &Path,
    config: &Config,
    environment: &Environment,
    now: UnixSeconds,
) -> Result<Vec<WorkloadOutcome>, tpt_profiler_model::Error> {
    if !config.profiling.runtime {
        return Ok(Vec::new());
    }
    let mut outcomes = Vec::with_capacity(config.workloads.len());
    for workload in &config.workloads {
        let mut runnable = RunnableWorkload::new(
            Workload {
                name: workload.name.clone(),
                input: None,
                duration_hint_ms: None,
                repetitions: workload.repetitions,
            },
            root.join(&workload.command),
        );
        runnable.args.clone_from(&workload.args);
        if let Some(warmup) = workload.warmup_runs {
            runnable.warmup_runs = warmup;
        }
        if let Some(timeout_ms) = workload.timeout_ms {
            runnable.timeout = Duration::from_millis(timeout_ms);
        }
        let result = measure_workload(&runnable, None)?;
        let measurements = to_measurements(
            &result,
            &MeasurementSource::WallClock,
            now,
            Some(environment),
        );
        outcomes.push(WorkloadOutcome {
            name: workload.name.clone(),
            measurements,
            samples: result.summary.n,
            failed_runs: result.failed_runs,
        });
    }
    Ok(outcomes)
}
