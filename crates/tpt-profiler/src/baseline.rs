//! Baselines and comparison (spec §36, §37): save a profiling snapshot
//! under `.tpt-profiler/`, compare later runs against it with
//! noise-aware assessment.
//!
//! Layout (git-friendly, spec §36, §69):
//!
//! ```text
//! .tpt-profiler/
//! └── baselines/
//!     └── <name>.json        (SchemaEnvelope<StoredBaseline>)
//! ```

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tpt_profiler_model::error::{Error, SerializationError};
use tpt_profiler_model::{Confidence, Environment, Measurement, ProfileRun, SchemaEnvelope};

use crate::config::Config;

/// Directory (relative to the workspace root) holding profiler state.
pub const STATE_DIR: &str = ".tpt-profiler";
/// Subdirectory holding named baselines.
pub const BASELINES_DIR: &str = "baselines";

/// A saved baseline: measurements plus the environment they were taken
/// in (spec §36, §49).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredBaseline {
    /// Baseline name (e.g. `main`, `pre-simd`).
    pub name: String,
    /// Unix timestamp of creation.
    pub created_at: i64,
    /// Environment snapshot (spec §49: comparisons must detect
    /// environment mismatch).
    pub environment: Environment,
    /// The measured facts being stored (spec §24).
    pub measurements: Vec<Measurement>,
}

/// Path of the baselines directory under a workspace root.
#[must_use]
pub fn baselines_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(STATE_DIR).join(BASELINES_DIR)
}

/// Saves measurements as a named baseline.
///
/// # Errors
/// Fails on filesystem errors or when the measurements cannot be
/// serialised (spec §74).
pub fn save_baseline(workspace_root: &Path, baseline: &StoredBaseline) -> Result<PathBuf, Error> {
    let dir = baselines_dir(workspace_root);
    std::fs::create_dir_all(&dir).map_err(|error| {
        Error::Serialization(SerializationError::EncodeFailed {
            message: format!("could not create {}: {error}", dir.display()),
        })
    })?;
    let path = dir.join(format!("{}.json", baseline.name));
    let envelope = SchemaEnvelope::new(
        format!("tpt-profiler {}", tpt_profiler_model::version()),
        baseline,
    );
    let json = serde_json::to_string_pretty(&envelope).map_err(|error| {
        Error::Serialization(SerializationError::EncodeFailed {
            message: error.to_string(),
        })
    })?;
    std::fs::write(&path, json).map_err(|error| {
        Error::Serialization(SerializationError::EncodeFailed {
            message: format!("could not write {}: {error}", path.display()),
        })
    })?;
    Ok(path)
}

/// Loads a named baseline; `None` when it does not exist (a missing
/// baseline is a state, not a crash — spec §74).
///
/// # Errors
/// Fails on unreadable files, corrupt JSON, or schema versions this
/// build cannot interpret.
pub fn load_baseline(workspace_root: &Path, name: &str) -> Result<Option<StoredBaseline>, Error> {
    let path = baselines_dir(workspace_root).join(format!("{name}.json"));
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|error| {
        Error::Serialization(SerializationError::DecodeFailed {
            message: format!("could not read {}: {error}", path.display()),
        })
    })?;
    let envelope: SchemaEnvelope<StoredBaseline> =
        serde_json::from_str(&text).map_err(|error| {
            Error::Serialization(SerializationError::DecodeFailed {
                message: format!("{} is corrupt: {error}", path.display()),
            })
        })?;
    envelope.validate().map_err(Error::Serialization)?;
    Ok(Some(envelope.data))
}

/// Assessment of one metric comparison (spec §36: "Performance Change").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assessment {
    /// Faster/smaller beyond the improvement margin.
    Improved,
    /// Slower/bigger beyond the configured threshold.
    Regressed,
    /// Within the noise margin.
    Unchanged,
    /// The baseline (or current) measurement is too noisy to judge.
    Noisy,
}

/// How far a regression overshoots its threshold (spec §37).
///
/// The tiers are multiples of the configured threshold, so a `10%`
/// threshold makes a `20%` change major and a `50%` change critical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegressionSeverity {
    /// Beyond the threshold, but less than [`MAJOR_MULTIPLE`] times it.
    Minor,
    /// At least [`MAJOR_MULTIPLE`] times the threshold.
    Major,
    /// At least [`CRITICAL_MULTIPLE`] times the threshold.
    Critical,
}

/// Multiple of the threshold at which a regression becomes major.
pub const MAJOR_MULTIPLE: f64 = 2.0;
/// Multiple of the threshold at which a regression becomes critical.
pub const CRITICAL_MULTIPLE: f64 = 5.0;

impl RegressionSeverity {
    /// Classifies a regression of `magnitude` (an absolute fractional
    /// change) against `threshold`.
    #[must_use]
    pub fn classify(magnitude: f64, threshold: f64) -> Self {
        if magnitude >= threshold * CRITICAL_MULTIPLE {
            Self::Critical
        } else if magnitude >= threshold * MAJOR_MULTIPLE {
            Self::Major
        } else {
            Self::Minor
        }
    }

    /// Lower-case label for reports (e.g. `major`).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Minor => "minor",
            Self::Major => "major",
            Self::Critical => "critical",
        }
    }
}

/// One metric's comparison result (spec §36, §37).
#[derive(Debug, Clone, PartialEq)]
pub struct MetricComparison {
    /// Metric name.
    pub metric: String,
    /// Baseline value.
    pub baseline: f64,
    /// Current value.
    pub current: f64,
    /// Signed fractional change (negative = lower = better for most
    /// metrics).
    pub change: f64,
    /// Whether lower values are better for this metric.
    pub lower_is_better: bool,
    /// The noise-aware judgement.
    pub assessment: Assessment,
    /// Set only for [`Assessment::Regressed`]: how far past the threshold
    /// the change went.
    pub severity: Option<RegressionSeverity>,
    /// Confidence in the judgement: the weaker of the baseline and current
    /// measurement confidences.
    pub confidence: Confidence,
}

impl MetricComparison {
    /// Percentage string for reports (e.g. `-18.4%`).
    #[must_use]
    pub fn change_percent(&self) -> String {
        format!("{:+.1}%", self.change * 100.0)
    }
}

/// Full comparison result (spec §36).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompareReport {
    /// Per-metric comparisons, sorted by metric name.
    pub metrics: Vec<MetricComparison>,
    /// Non-empty when baseline and current environments differ
    /// (spec §36: warn on environment mismatch).
    pub environment_warning: Option<String>,
}

/// Metrics where a higher value is better; everything else (times,
/// memory, allocation counts) is lower-is-better.
fn higher_is_better(metric: &str) -> bool {
    metric.contains("throughput") || metric.contains("ops_per") || metric.contains("mibs_per")
}

/// Compares a baseline against the current run, using the configured
/// thresholds and the confidence of both measurements (noise-aware:
/// `Low`-confidence pairs are reported as [`Assessment::Noisy`], never
/// as regressions — spec §50, §37).
#[must_use]
pub fn compare(baseline: &StoredBaseline, current: &ProfileRun, config: &Config) -> CompareReport {
    let mut report = CompareReport::default();
    if environment_mismatch(&baseline.environment, &current.environment) {
        report.environment_warning = Some(String::from(
            "baseline and current runs were captured on different environments \
             (OS/arch/toolchain); treat changes with care (spec §36)",
        ));
    }
    let mut comparisons = Vec::new();
    for current_measurement in &current.measurements {
        let Some(baseline_measurement) = baseline
            .measurements
            .iter()
            .find(|candidate| candidate.metric == current_measurement.metric)
        else {
            continue;
        };
        if baseline_measurement.value <= 0.0 {
            continue;
        }
        let change =
            (current_measurement.value - baseline_measurement.value) / baseline_measurement.value;
        let lower_is_better = !higher_is_better(&current_measurement.metric);
        let noisy = baseline_measurement.confidence == Confidence::Low
            || current_measurement.confidence == Confidence::Low;
        let threshold = if current_measurement
            .metric
            .starts_with(crate::measure::BINARY_METRIC_PREFIX)
        {
            config.ci.binary_threshold
        } else {
            config.ci.runtime_threshold
        };
        let assessment = if noisy {
            Assessment::Noisy
        } else if lower_is_better {
            if change > threshold {
                Assessment::Regressed
            } else if change < -threshold {
                Assessment::Improved
            } else {
                Assessment::Unchanged
            }
        } else if change < -threshold {
            Assessment::Regressed
        } else if change > threshold {
            Assessment::Improved
        } else {
            Assessment::Unchanged
        };
        let severity = (assessment == Assessment::Regressed)
            .then(|| RegressionSeverity::classify(change.abs(), threshold));
        comparisons.push(MetricComparison {
            metric: current_measurement.metric.clone(),
            baseline: baseline_measurement.value,
            current: current_measurement.value,
            change,
            lower_is_better,
            assessment,
            severity,
            confidence: baseline_measurement
                .confidence
                .min(current_measurement.confidence),
        });
    }
    comparisons.sort_by(|a, b| a.metric.cmp(&b.metric));
    report.metrics = comparisons;
    report
}

fn environment_mismatch(a: &Environment, b: &Environment) -> bool {
    match (&a.os, &b.os) {
        (Some(a_os), Some(b_os)) => a_os != b_os,
        _ => false, // insufficient data to claim a mismatch
    }
}

/// Extracts the comparable measurements from a run.
///
/// Every measurement the collectors emit is already a comparable value (a
/// mean, a size, a throughput), so all of them are kept. Statistical
/// summaries such as sample counts stay in the run record. Deterministic
/// values such as binary sizes were once dropped here, which silently hid
/// them from `compare`.
#[must_use]
pub fn baseline_from_run(name: &str, run: &ProfileRun) -> StoredBaseline {
    StoredBaseline {
        name: String::from(name),
        created_at: run.started_at,
        environment: run.environment.clone(),
        measurements: run.measurements.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_profiler_model::SCHEMA_VERSION;
    use tpt_profiler_model::{MeasurementSource, Unit};

    fn measurement(metric: &str, value: f64, confidence: Confidence) -> Measurement {
        let mut measurement = Measurement::runtime(
            metric,
            value,
            Unit::Seconds,
            MeasurementSource::WallClock,
            1_760_000_000,
        );
        measurement.confidence = confidence;
        measurement
    }

    fn baseline_with(values: &[(&str, f64, Confidence)]) -> StoredBaseline {
        StoredBaseline {
            name: String::from("test"),
            created_at: 1_760_000_000,
            environment: Environment::default(),
            measurements: values
                .iter()
                .map(|(metric, value, confidence)| measurement(metric, *value, *confidence))
                .collect(),
        }
    }

    fn run_with(values: &[(&str, f64, Confidence)]) -> ProfileRun {
        let mut run = ProfileRun::new("run-2", 1_760_000_100, Environment::default());
        run.measurements = values
            .iter()
            .map(|(metric, value, confidence)| measurement(metric, *value, *confidence))
            .collect();
        run
    }

    #[test]
    fn save_and_load_round_trip_with_schema_check() {
        let temp = std::env::temp_dir().join("tpt-baseline-roundtrip");
        std::fs::remove_dir_all(&temp).ok();
        let baseline = baseline_with(&[("wall_time", 1.5, Confidence::High)]);
        let path = save_baseline(&temp, &baseline).expect("save");
        assert!(path.ends_with(".tpt-profiler/baselines/test.json"));

        let loaded = load_baseline(&temp, "test")
            .expect("readable")
            .expect("exists");
        assert_eq!(loaded, baseline);
        assert!(
            std::fs::read_to_string(&path)
                .expect("read")
                .contains(&format!("\"schema_version\": {SCHEMA_VERSION}"))
        );
        std::fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn missing_baseline_is_none_not_an_error() {
        let temp = std::env::temp_dir().join("tpt-baseline-missing");
        std::fs::create_dir_all(&temp).ok();
        assert!(load_baseline(&temp, "nope").expect("ok").is_none());
        std::fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn corrupt_baselines_are_rejected() {
        let temp = std::env::temp_dir().join("tpt-baseline-corrupt");
        let dir = baselines_dir(&temp);
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("bad.json"), "{ not json").expect("write");
        assert!(load_baseline(&temp, "bad").is_err());
        std::fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn future_schema_versions_are_rejected_not_reinterpreted() {
        let temp = std::env::temp_dir().join("tpt-baseline-future");
        let dir = baselines_dir(&temp);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let baseline = baseline_with(&[("wall_time", 1.0, Confidence::High)]);
        let json = serde_json::json!({
            "schema_version": SCHEMA_VERSION + 1,
            "generated_by": "from the future",
            "data": serde_json::to_value(&baseline).expect("baseline value"),
        });
        std::fs::write(
            dir.join("future.json"),
            serde_json::to_string(&json).expect("encode"),
        )
        .expect("write");
        let error = load_baseline(&temp, "future").expect_err("must reject");
        assert!(
            matches!(
                error,
                Error::Serialization(SerializationError::SchemaTooNew { .. })
            ),
            "{error}"
        );
        std::fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn compare_flags_regressions_and_improvements() {
        let config = Config::default(); // runtime_threshold 0.10
        let baseline = baseline_with(&[
            ("decode_wall_time", 10.0, Confidence::High),
            ("encode_wall_time", 10.0, Confidence::High),
            ("load_wall_time", 10.0, Confidence::High),
        ]);
        let current = run_with(&[
            ("decode_wall_time", 8.16, Confidence::High), // -18.4% improved
            ("encode_wall_time", 11.1, Confidence::High), // +11% > 10% threshold: regressed
            ("load_wall_time", 10.3, Confidence::High),   // +3% unchanged
        ]);
        let report = compare(&baseline, &current, &config);
        assert_eq!(report.metrics.len(), 3);
        assert_eq!(report.metrics[0].assessment, Assessment::Improved);
        assert_eq!(report.metrics[1].assessment, Assessment::Regressed);
        assert_eq!(report.metrics[2].assessment, Assessment::Unchanged);
        assert_eq!(report.metrics[0].change_percent(), "-18.4%");
    }

    #[test]
    fn low_confidence_pairs_are_judged_noisy_never_regressed() {
        let config = Config::default();
        let baseline = baseline_with(&[("decode_wall_time", 10.0, Confidence::Low)]);
        let current = run_with(&[("decode_wall_time", 50.0, Confidence::High)]);
        let report = compare(&baseline, &current, &config);
        assert_eq!(report.metrics[0].assessment, Assessment::Noisy);
    }

    #[test]
    fn regressions_are_graded_by_multiple_of_threshold() {
        let config = Config::default(); // runtime_threshold 0.10
        let baseline = baseline_with(&[
            ("minor_wall_time", 10.0, Confidence::High),
            ("major_wall_time", 10.0, Confidence::High),
            ("critical_wall_time", 10.0, Confidence::High),
            ("unchanged_wall_time", 10.0, Confidence::High),
        ]);
        let current = run_with(&[
            ("minor_wall_time", 11.5, Confidence::High), // +15%: 1.5x threshold
            ("major_wall_time", 12.5, Confidence::High), // +25%: 2.5x threshold
            ("critical_wall_time", 16.0, Confidence::High), // +60%: 6x threshold
            ("unchanged_wall_time", 10.5, Confidence::High),
        ]);
        let report = compare(&baseline, &current, &config);
        let severity = |name: &str| {
            report
                .metrics
                .iter()
                .find(|metric| metric.metric == name)
                .expect("present")
                .severity
        };
        assert_eq!(severity("minor_wall_time"), Some(RegressionSeverity::Minor));
        assert_eq!(severity("major_wall_time"), Some(RegressionSeverity::Major));
        assert_eq!(
            severity("critical_wall_time"),
            Some(RegressionSeverity::Critical)
        );
        assert_eq!(severity("unchanged_wall_time"), None);
    }

    #[test]
    fn regression_confidence_is_the_weaker_side() {
        let config = Config::default();
        let baseline = baseline_with(&[("decode_wall_time", 10.0, Confidence::High)]);
        let current = run_with(&[("decode_wall_time", 11.5, Confidence::Medium)]);
        let report = compare(&baseline, &current, &config);
        assert_eq!(report.metrics[0].assessment, Assessment::Regressed);
        assert_eq!(report.metrics[0].confidence, Confidence::Medium);
        assert_eq!(report.metrics[0].severity, Some(RegressionSeverity::Minor));
    }

    #[test]
    fn environment_mismatch_is_warned_about() {
        let config = Config::default();
        let mut baseline = baseline_with(&[("decode_wall_time", 10.0, Confidence::High)]);
        baseline.environment.os = Some(String::from("linux"));
        let mut run = run_with(&[("decode_wall_time", 10.0, Confidence::High)]);
        run.environment.os = Some(String::from("windows"));
        let report = compare(&baseline, &run, &config);
        assert!(report.environment_warning.is_some());
    }
}
