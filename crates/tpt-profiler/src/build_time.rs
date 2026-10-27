//! Build timing for `tpt-profiler build` (spec §8.6, §20).
//!
//! Two builds run in a target directory this tool owns
//! (`.tpt-profiler/build-target`), so the user's own `target/` is never
//! cleared. The first build starts from an empty directory and gives the
//! clean time and the per-crate report. The second follows a touch of one
//! workspace crate's root source file and gives the incremental time. The
//! touch changes only the file's modification time, which is restored
//! afterwards; the file's contents are never written.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use cargo_metadata::TargetKind;
use tpt_profiler_build::{
    BuildFinding, BuildSummary, findings, normalise_name, parse_timing_report, summarise,
};
use tpt_profiler_model::error::{ProfilingError, ToolUnavailableError};
use tpt_profiler_model::{
    Confidence, Error, FindingKind, Measurement, MeasurementSource, Unit, UnixSeconds,
};

/// Upper bound for one `cargo build` in the timing run.
const BUILD_TIMEOUT: Duration = Duration::from_secs(3600);

/// Target directory, relative to the repository root, that timing builds use.
pub const BUILD_TARGET_DIR: &str = ".tpt-profiler/build-target";

/// Where cargo writes the timing report inside a target directory.
const TIMING_REPORT: &str = "cargo-timings/cargo-timing.html";

/// Metric for the clean build, in seconds.
pub const CLEAN_METRIC: &str = "build:clean_seconds";

/// Metric for the incremental build, in seconds.
pub const INCREMENTAL_METRIC: &str = "build:incremental_seconds";

/// Result of timing the workspace's builds (spec §20).
#[derive(Debug, Clone, PartialEq)]
pub struct BuildOutcome {
    /// Wall-clock time of the clean build.
    pub clean_seconds: f64,
    /// Wall-clock time of the incremental build, when a workspace crate
    /// had a source file to touch.
    pub incremental_seconds: Option<f64>,
    /// The workspace crate whose source was touched for the incremental build.
    pub touched_crate: Option<String>,
    /// Per-crate and per-kind cost from the clean build's timing report.
    pub summary: BuildSummary,
    /// Findings where build cost is concentrated.
    pub findings: Vec<BuildFinding>,
    /// Measurements for the baseline and comparison machinery.
    pub measurements: Vec<Measurement>,
}

/// Times a clean and an incremental `cargo build --workspace` (spec §20).
///
/// # Errors
/// Fails when `cargo metadata` or `cargo` is unavailable, when a build fails
/// or times out, or when the timing report is missing or malformed.
pub fn measure_builds(root: &Path, now: UnixSeconds) -> Result<BuildOutcome, Error> {
    let metadata = crate::discovery::load_metadata(root)?;
    let proc_macros: BTreeSet<String> = metadata
        .packages
        .iter()
        .filter(|package| {
            package
                .targets
                .iter()
                .any(|target| target.kind.contains(&TargetKind::ProcMacro))
        })
        .map(|package| normalise_name(package.name.as_ref()))
        .collect();
    let touch = touch_candidate(&metadata);

    let target_dir = root.join(BUILD_TARGET_DIR);
    if target_dir.exists() {
        std::fs::remove_dir_all(&target_dir)
            .map_err(|error| filesystem_failure(&target_dir, &error))?;
    }
    let clean_seconds = timed_build(root, &target_dir)?;

    let report_path = target_dir.join(TIMING_REPORT);
    let html = std::fs::read_to_string(&report_path)
        .map_err(|error| filesystem_failure(&report_path, &error))?;
    let units = parse_timing_report(&html)?;
    let summary = summarise(&units, &proc_macros);
    let found = findings(&summary);

    let (incremental_seconds, touched_crate) = match touch {
        Some((name, path)) => (Some(touched_build(root, &target_dir, &path)?), Some(name)),
        None => (None, None),
    };

    let mut measurements = vec![timing_measurement(CLEAN_METRIC, clean_seconds, now)];
    if let Some(seconds) = incremental_seconds {
        measurements.push(timing_measurement(INCREMENTAL_METRIC, seconds, now));
    }
    Ok(BuildOutcome {
        clean_seconds,
        incremental_seconds,
        touched_crate,
        summary,
        findings: found,
        measurements,
    })
}

/// Picks the first workspace library or binary, returning its package name
/// and root source file.
fn touch_candidate(metadata: &cargo_metadata::Metadata) -> Option<(String, PathBuf)> {
    metadata
        .packages
        .iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
        .find_map(|package| {
            package
                .targets
                .iter()
                .find(|target| {
                    target
                        .kind
                        .iter()
                        .any(|kind| matches!(kind, TargetKind::Lib | TargetKind::Bin))
                })
                .map(|target| {
                    (
                        package.name.to_string(),
                        target.src_path.clone().into_std_path_buf(),
                    )
                })
        })
}

/// Runs one `cargo build --workspace --timings` into `target_dir` and
/// returns its wall-clock time in seconds.
fn timed_build(root: &Path, target_dir: &Path) -> Result<f64, Error> {
    let label = "cargo build --workspace --timings";
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .env("CARGO_TARGET_DIR", target_dir)
        .args(["build", "--workspace", "--timings"]);
    let outcome =
        tpt_profiler_runtime::process::run(&mut command, BUILD_TIMEOUT, None).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::ToolUnavailable(ToolUnavailableError::Tool {
                    tool: String::from("cargo"),
                    hint: String::from("cargo must be on PATH to time builds"),
                })
            } else {
                Error::Profiling(ProfilingError::SpawnFailed {
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
    Ok(outcome.wall_time.as_secs_f64())
}

/// Touches `source` so the crate rebuilds, times the build, and restores
/// the original modification time whether or not the build succeeded.
fn touched_build(root: &Path, target_dir: &Path, source: &Path) -> Result<f64, Error> {
    let original = std::fs::metadata(source)
        .and_then(|metadata| metadata.modified())
        .map_err(|error| filesystem_failure(source, &error))?;
    set_modified(source, SystemTime::now())?;
    let timed = timed_build(root, target_dir);
    let restored = set_modified(source, original);
    let seconds = timed?;
    restored?;
    Ok(seconds)
}

/// Sets a file's modification time without touching its contents.
fn set_modified(path: &Path, time: SystemTime) -> Result<(), Error> {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(time))
        .map_err(|error| filesystem_failure(path, &error))
}

fn filesystem_failure(path: &Path, error: &std::io::Error) -> Error {
    Error::Profiling(ProfilingError::BuildFailed {
        command: String::from("cargo build --timings"),
        message: format!("`{}`: {error}", path.display()),
    })
}

/// A single build time is one execution, so it is at most `Low` confidence
/// (spec §50); it is a timing, so it is statistical (spec §75).
fn timing_measurement(metric: &str, seconds: f64, now: UnixSeconds) -> Measurement {
    Measurement {
        metric: metric.to_owned(),
        value: seconds,
        unit: Unit::Seconds,
        source: MeasurementSource::BuildTiming,
        timestamp: now,
        workload: None,
        environment: None,
        confidence: Confidence::Low,
        kind: FindingKind::Statistical,
    }
}
