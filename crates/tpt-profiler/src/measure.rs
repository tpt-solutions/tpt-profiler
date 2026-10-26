//! Runtime measurement collection shared by `runtime`, `baseline`,
//! `compare` and `ci` (spec §8.2, §23, §36, §39).
//!
//! Workloads run here, once, so every consumer sees the same metric names
//! and the same statistics. Without this, a baseline or a CI comparison
//! would contain only static findings and could never judge a runtime
//! change.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use tpt_profiler_model::error::{ProfilingError, ToolUnavailableError};
use tpt_profiler_model::{
    Confidence, Environment, FindingKind, Measurement, MeasurementSource, Unit, UnixSeconds,
    Workload, hierarchy::CrateInfo,
};
use tpt_profiler_runtime::perf::{self, Hotspot, Perf};
use tpt_profiler_runtime::{
    Platform, RunnableWorkload, measure_workload, select_backend, to_measurements,
};

use crate::config::Config;

/// Upper bound for one `cargo bench` invocation (spec §8.3).
const BENCH_TIMEOUT: Duration = Duration::from_secs(1800);

/// Upper bound for one release build (spec §19).
const BUILD_TIMEOUT: Duration = Duration::from_secs(1800);

/// Metric-name prefix for binary sizes. `compare` and the CI policy use it to
/// recognise size metrics, which have their own threshold (spec §19, §39).
pub const BINARY_METRIC_PREFIX: &str = "binary:";

/// Converts a byte count to `f64`. Binaries are far below 2^53 bytes, where
/// `f64` stops representing every integer exactly, so the conversion is exact.
#[allow(clippy::cast_precision_loss)]
fn size_as_f64(bytes: u64) -> f64 {
    bytes as f64
}

/// Builds the workspace's release binaries and records each one's size in
/// bytes (spec §19). Metric names are `binary:<target>_bytes`.
///
/// Returns an empty list unless `[profiling] binary` is enabled. Sizes are
/// deterministic for a given toolchain and source, so their confidence is
/// `High` and `compare` can judge them without repeated runs.
///
/// # Errors
/// Fails when `cargo` is missing, the release build fails or times out, or a
/// reported binary is absent afterwards.
pub fn collect_binaries(
    root: &Path,
    config: &Config,
    now: UnixSeconds,
) -> Result<Vec<Measurement>, tpt_profiler_model::Error> {
    if !config.profiling.binary {
        return Ok(Vec::new());
    }
    let label = "cargo build --release";
    let mut command = Command::new("cargo");
    command.current_dir(root).args([
        "build",
        "--release",
        "--workspace",
        "--bins",
        "--message-format=json",
    ]);
    let outcome =
        tpt_profiler_runtime::process::run(&mut command, BUILD_TIMEOUT, None).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                tpt_profiler_model::Error::ToolUnavailable(ToolUnavailableError::Tool {
                    tool: String::from("cargo"),
                    hint: String::from("cargo must be on PATH to build release binaries"),
                })
            } else {
                tpt_profiler_model::Error::Profiling(ProfilingError::SpawnFailed {
                    command: label.to_owned(),
                    message: error.to_string(),
                })
            }
        })?;
    if outcome.timed_out {
        return Err(ProfilingError::TimedOut {
            command: label.to_owned(),
            timeout_ms: u64::try_from(BUILD_TIMEOUT.as_millis()).unwrap_or(u64::MAX),
        }
        .into());
    }
    if !outcome.success {
        return Err(ProfilingError::BuildFailed {
            command: label.to_owned(),
            message: format!("exit code {:?}\n{}", outcome.exit_code, outcome.stderr),
        }
        .into());
    }

    let mut measurements = Vec::new();
    // `--message-format=json` emits one JSON object per line; only
    // `compiler-artifact` messages for binary targets carry an executable.
    for line in outcome.stdout.lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if message["reason"] != "compiler-artifact" {
            continue;
        }
        let (Some(executable), Some(target)) = (
            message["executable"].as_str(),
            message["target"]["name"].as_str(),
        ) else {
            continue;
        };
        let metadata =
            std::fs::metadata(executable).map_err(|error| ProfilingError::BuildFailed {
                command: label.to_owned(),
                message: format!("built binary `{executable}` is missing: {error}"),
            })?;
        measurements.push(Measurement {
            metric: format!("{BINARY_METRIC_PREFIX}{target}_bytes"),
            value: size_as_f64(metadata.len()),
            unit: Unit::Bytes,
            source: MeasurementSource::BinarySize,
            timestamp: now,
            workload: None,
            environment: None,
            confidence: Confidence::High,
            kind: FindingKind::Deterministic,
        });
    }
    Ok(measurements)
}

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

/// Runs of each workload under `perf stat`. Counters are means over these
/// runs, so they are statistical evidence rather than one definitive run
/// (spec §50).
pub const COUNTER_REPEATS: u32 = 3;

/// Sampling frequency for `perf record`, in Hz. Odd, so it does not
/// lock-step with periodic work in the program.
pub const SAMPLE_HZ: u32 = 499;

/// Upper bound for the `perf` availability probe.
const PERF_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Per-run timeout for a workload that does not set `timeout_ms`.
const DEFAULT_WORKLOAD_TIMEOUT: Duration = Duration::from_secs(60);

/// What `perf` observed for one configured workload (spec §8.5).
#[derive(Debug, Clone, PartialEq)]
pub struct CounterOutcome {
    /// Workload name from `[[workloads]]`.
    pub name: String,
    /// Mean hardware-counter values as measurements.
    pub measurements: Vec<Measurement>,
    /// Leaf-frame hotspots from sampling, ranked by sample count.
    pub hotspots: Vec<Hotspot>,
    /// The workload failed or timed out, so nothing was counted.
    pub failed: bool,
}

/// Runs every configured workload under Linux `perf` (spec §8.5): hardware
/// counters through `perf stat`, and sampled hotspots through `perf record`.
///
/// # Errors
/// Returns [`ToolUnavailableError`] when `perf` is missing or this platform
/// has no backend, so the caller can say so and carry on (spec §74).
pub fn collect_counters(
    root: &Path,
    config: &Config,
    environment: &Environment,
    now: UnixSeconds,
) -> Result<Vec<CounterOutcome>, tpt_profiler_model::Error> {
    let perf = Perf;
    let Some(platform) = Platform::current() else {
        return Err(perf_unavailable(String::from(
            "perf is only supported on Linux",
        )));
    };
    select_backend(&[&perf], platform, PERF_PROBE_TIMEOUT).map_err(|reasons| {
        let hint = reasons.first().map_or_else(
            || String::from("perf is only supported on Linux"),
            ToString::to_string,
        );
        perf_unavailable(hint)
    })?;

    let mut outcomes = Vec::with_capacity(config.workloads.len());
    for (index, workload) in config.workloads.iter().enumerate() {
        let program = root.join(&workload.command);
        let per_run = workload
            .timeout_ms
            .map_or(DEFAULT_WORKLOAD_TIMEOUT, Duration::from_millis);

        let mut stat = perf_command(
            perf::stat_args(perf::HARDWARE_EVENTS, COUNTER_REPEATS),
            &program,
            &workload.args,
        );
        let counted = run_perf(
            &mut stat,
            per_run * COUNTER_REPEATS,
            &format!("perf stat {}", workload.name),
        )?;
        let failed = counted.timed_out || !counted.success;
        let counters = if failed {
            Vec::new()
        } else {
            perf::parse_stat(&counted.stderr)
        };
        let measurements = counters
            .iter()
            .map(|counter| {
                let mut measurement = Measurement::runtime(
                    format!("{}_{}", workload.name, counter.event.replace('-', "_")),
                    counter.value,
                    Unit::Count,
                    MeasurementSource::Perf,
                    now,
                );
                measurement.workload = Some(workload.name.clone());
                measurement.environment = Some(environment.clone());
                measurement.confidence = Confidence::Medium;
                measurement
            })
            .collect();

        let hotspots = if failed {
            Vec::new()
        } else {
            sample_hotspots(&program, &workload.args, per_run, index)?
        };
        outcomes.push(CounterOutcome {
            name: workload.name.clone(),
            measurements,
            hotspots,
            failed,
        });
    }
    Ok(outcomes)
}

/// Records one sampled run and reads its leaf frames. A recording that
/// cannot be made or read yields no hotspots rather than failing the run.
fn sample_hotspots(
    program: &Path,
    args: &[String],
    timeout: Duration,
    index: usize,
) -> Result<Vec<Hotspot>, tpt_profiler_model::Error> {
    let recording =
        std::env::temp_dir().join(format!("tpt-perf-{}-{index}.data", std::process::id()));
    let recording_text = recording.to_string_lossy().into_owned();
    let mut record = perf_command(perf::record_args(&recording_text, SAMPLE_HZ), program, args);
    let recorded = run_perf(&mut record, timeout, "perf record")?;
    let hotspots = if recorded.success && !recorded.timed_out {
        let mut script = Command::new("perf");
        script.args(perf::script_args(&recording_text));
        let output = run_perf(&mut script, timeout, "perf script")?;
        if output.success {
            perf::parse_script(&output.stdout)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    let _ = std::fs::remove_file(&recording);
    Ok(hotspots)
}

/// `perf <prefix…> <program> <args…>` as one command.
fn perf_command(prefix: Vec<String>, program: &Path, args: &[String]) -> Command {
    let mut command = Command::new("perf");
    command.args(prefix).arg(program).args(args);
    command
}

/// Runs a `perf` command, mapping spawn failures onto the error taxonomy.
fn run_perf(
    command: &mut Command,
    timeout: Duration,
    label: &str,
) -> Result<tpt_profiler_runtime::RunOutcome, tpt_profiler_model::Error> {
    tpt_profiler_runtime::process::run(command, timeout, None).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            perf_unavailable(String::from("`perf` was not found on PATH"))
        } else {
            tpt_profiler_model::Error::Profiling(ProfilingError::SpawnFailed {
                command: label.to_owned(),
                message: error.to_string(),
            })
        }
    })
}

fn perf_unavailable(hint: String) -> tpt_profiler_model::Error {
    tpt_profiler_model::Error::ToolUnavailable(ToolUnavailableError::Tool {
        tool: String::from("perf"),
        hint,
    })
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
