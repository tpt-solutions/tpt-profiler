//! Workload execution and measurement (spec §8.2, §23, §50).

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use tpt_profiler_model::{
    Environment, FindingKind, Measurement, MeasurementSource, Unit, UnixSeconds, Workload,
};

use crate::process::{CancelToken, RunOutcome};
use crate::stats::Summary;

/// A runnable workload definition: the model [`Workload`] plus the actual
/// command to execute (the spec's `input` is the command's argument
/// surface).
#[derive(Debug, Clone)]
pub struct RunnableWorkload {
    /// The modelled workload (name, repetitions, duration hint).
    pub workload: Workload,
    /// Executable to run.
    pub program: PathBuf,
    /// Arguments (often the input fixture path from the workload).
    pub args: Vec<String>,
    /// Warm-up runs discarded before measuring (spec §50).
    pub warmup_runs: u32,
    /// Per-run timeout.
    pub timeout: Duration,
}

impl RunnableWorkload {
    /// A runnable workload with sensible defaults: two warm-ups, the
    /// workload's repetition count (default 5), and a 60-second timeout.
    #[must_use]
    pub fn new(workload: Workload, program: impl Into<PathBuf>) -> Self {
        Self {
            workload,
            program: program.into(),
            args: Vec::new(),
            warmup_runs: 2,
            timeout: Duration::from_secs(60),
        }
    }
}

/// Result of measuring one workload.
#[derive(Debug, Clone)]
pub struct WorkloadResult {
    /// Workload name.
    pub workload_name: String,
    /// Statistical summary over retained wall-time samples.
    pub summary: Summary,
    /// Cold-start sample (first-ever execution), when requested.
    pub cold_start: Option<f64>,
    /// Raw retained samples in seconds.
    pub samples_seconds: Vec<f64>,
    /// Number of runs that failed or timed out (spec §74: recorded, not
    /// silently swallowed).
    pub failed_runs: u32,
}

/// Measures one workload by running it repeatedly and summarising wall
/// time (spec §50: repeats, warm-up, never a single definitive run).
pub fn measure_workload(
    runnable: &RunnableWorkload,
    cancel: Option<&CancelToken>,
) -> Result<WorkloadResult, tpt_profiler_model::error::Error> {
    let repetitions = runnable.workload.repetitions.unwrap_or(5).max(1);
    let run = |attempt: u32| -> Result<RunOutcome, tpt_profiler_model::error::Error> {
        let _ = attempt;
        let mut command = Command::new(&runnable.program);
        command.args(&runnable.args);
        crate::process::run(&mut command, runnable.timeout, cancel)
            .map_err(|error| spawn_error(&error))
    };

    // Cold start: recorded but never averaged in (spec §50).
    let cold_start = if runnable.warmup_runs == 0 {
        None
    } else {
        let started = Instant::now();
        run(0)?;
        Some(started.elapsed().as_secs_f64())
    };

    for attempt in 1..=runnable.warmup_runs {
        run(attempt)?;
    }

    let mut samples = Vec::with_capacity(repetitions as usize);
    let mut failed_runs = 0_u32;
    for attempt in 0..repetitions {
        match run(runnable.warmup_runs + 1 + attempt) {
            Ok(outcome) if outcome.success && !outcome.timed_out => {
                samples.push(outcome.wall_time.as_secs_f64());
            }
            Ok(outcome) => {
                if outcome.cancelled {
                    return Err(tpt_profiler_model::error::Error::from(
                        tpt_profiler_model::error::ProfilingError::Cancelled,
                    ));
                }
                failed_runs += 1;
            }
            Err(error) => return Err(error),
        }
    }

    let summary = Summary::from_samples(&samples).ok_or_else(|| {
        // No usable samples: the run produced nothing measurable.
        tpt_profiler_model::error::Error::from(tpt_profiler_model::error::ProfilingError::Cancelled)
    })?;
    Ok(WorkloadResult {
        workload_name: runnable.workload.name.clone(),
        summary,
        cold_start,
        samples_seconds: samples,
        failed_runs,
    })
}

/// Converts a [`WorkloadResult`] into model measurements (spec §24):
/// the mean wall time plus the cold-start observation when present.
/// All runtime measurements are [`FindingKind::Statistical`] and inherit
/// their confidence from the sample spread (spec §75).
#[must_use]
pub fn to_measurements(
    result: &WorkloadResult,
    source: &MeasurementSource,
    now: UnixSeconds,
    environment: Option<&Environment>,
) -> Vec<Measurement> {
    let mut measurements = Vec::new();
    let mut mean = Measurement::runtime(
        format!("{}_wall_time", result.workload_name),
        result.summary.mean,
        Unit::Seconds,
        source.clone(),
        now,
    );
    mean.confidence = result.summary.confidence();
    mean.kind = FindingKind::Statistical;
    mean.workload = Some(result.workload_name.clone());
    if let Some(environment) = environment {
        mean.environment = Some(environment.clone());
    }
    measurements.push(mean);

    if let Some(cold_start) = result.cold_start {
        let mut cold = Measurement::runtime(
            format!("{}_cold_start", result.workload_name),
            cold_start,
            Unit::Seconds,
            MeasurementSource::WallClock,
            now,
        );
        cold.confidence = tpt_profiler_model::Confidence::Low;
        cold.kind = FindingKind::Statistical;
        cold.workload = Some(result.workload_name.clone());
        measurements.push(cold);
    }
    measurements
}

fn spawn_error(error: &std::io::Error) -> tpt_profiler_model::error::Error {
    tpt_profiler_model::error::Error::Profiling(
        tpt_profiler_model::error::ProfilingError::SpawnFailed {
            command: String::from("<workload>"),
            message: error.to_string(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurements_carry_statistical_kind_and_workload() {
        let result = WorkloadResult {
            workload_name: String::from("decode_1080p"),
            summary: Summary::from_samples(&[1.0, 1.01, 0.99, 1.0]).expect("samples"),
            cold_start: Some(3.0),
            samples_seconds: vec![1.0, 1.01, 0.99, 1.0],
            failed_runs: 0,
        };
        let measurements =
            to_measurements(&result, &MeasurementSource::WallClock, 1_760_000_000, None);
        assert_eq!(measurements.len(), 2, "mean + cold start");
        assert_eq!(measurements[0].metric, "decode_1080p_wall_time");
        assert_eq!(measurements[0].kind, FindingKind::Statistical);
        assert_eq!(measurements[0].workload.as_deref(), Some("decode_1080p"));
        assert!(measurements[0].confidence != tpt_profiler_model::Confidence::Low);
        assert_eq!(measurements[1].metric, "decode_1080p_cold_start");
    }

    #[test]
    fn environment_is_attached_when_given() {
        let result = WorkloadResult {
            workload_name: String::from("w"),
            summary: Summary::from_samples(&[1.0, 1.0]).expect("samples"),
            cold_start: None,
            samples_seconds: vec![1.0, 1.0],
            failed_runs: 0,
        };
        let mut environment = Environment::default();
        environment.cpu_cores = Some(8);
        let measurements =
            to_measurements(&result, &MeasurementSource::WallClock, 0, Some(&environment));
        assert_eq!(
            measurements[0]
                .environment
                .as_ref()
                .and_then(|environment| environment.cpu_cores),
            Some(8)
        );
    }

    #[test]
    fn runnable_defaults_are_sensible() {
        #[cfg(windows)]
        let program = PathBuf::from("cmd");
        #[cfg(not(windows))]
        let program = PathBuf::from("echo");
        let runnable = RunnableWorkload::new(Workload::named("x"), program);
        assert_eq!(runnable.warmup_runs, 2);
        assert_eq!(runnable.workload.repetitions, None);
        assert_eq!(runnable.timeout, Duration::from_secs(60));
    }
}
