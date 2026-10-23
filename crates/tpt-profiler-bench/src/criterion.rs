//! Criterion result ingestion (spec §8.3).
//!
//! Criterion writes `target/criterion/<group>/<bench>/new/estimates.json`
//! with point estimates and confidence intervals. This module walks that
//! tree and converts each benchmark into a [`Measurement`]; a missing or
//! empty tree is a *result* (no benchmarks ran), reported via
//! [`CriterionDataMissing`], not a crash (spec §74).
//!
//! [`CriterionDataMissing`]: tpt_profiler_model::error::BenchmarkError::CriterionDataMissing

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use tpt_profiler_model::{FindingKind, Measurement, MeasurementSource, Unit, UnixSeconds};
use tpt_profiler_runtime::stats::confidence_for_cv;

/// Relative half-width of Criterion's 95% interval on the mean: how precisely
/// the mean is known, as a fraction of the mean. This is the quantity a
/// baseline comparison depends on. Per-sample spread (`std_dev`) is not used
/// because it mostly reflects run-to-run jitter, which the mean's interval
/// already accounts for. A non-positive estimate is treated as infinitely
/// imprecise.
fn relative_spread(estimate: &CriterionEstimate) -> f64 {
    if estimate.point_estimate > 0.0 {
        (estimate.upper_bound - estimate.lower_bound) / 2.0 / estimate.point_estimate
    } else {
        f64::INFINITY
    }
}

/// One parsed Criterion benchmark estimate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CriterionEstimate {
    /// Point estimate (e.g. mean) in nanoseconds.
    pub point_estimate: f64,
    /// Lower bound of Criterion's own confidence interval, ns.
    pub lower_bound: f64,
    /// Upper bound of Criterion's confidence interval, ns.
    pub upper_bound: f64,
    /// Standard deviation, ns.
    pub standard_deviation: f64,
}

/// Criterion `estimates.json` shape (only the fields we need; unknown
/// fields are ignored by serde). Criterion writes one top-level block per
/// statistic (`mean`, `median`, `median_abs_dev`, `slope`, `std_dev`), each
/// with a `point_estimate`, a `standard_error` and a confidence interval.
#[derive(Debug, Deserialize)]
struct Estimates {
    #[serde(default)]
    mean: Option<EstimateBlock>,
    #[serde(default)]
    median: Option<EstimateBlock>,
    #[serde(default)]
    std_dev: Option<EstimateBlock>,
}

#[derive(Debug, Deserialize)]
struct EstimateBlock {
    #[serde(rename = "confidence_interval")]
    confidence_interval: ConfidenceInterval,
    point_estimate: f64,
    #[serde(default)]
    standard_error: f64,
}

#[derive(Debug, Deserialize)]
struct ConfidenceInterval {
    #[serde(rename = "confidence_level")]
    #[allow(dead_code)]
    confidence_level: f64,
    #[serde(rename = "lower_bound")]
    lower_bound: f64,
    #[serde(rename = "upper_bound")]
    upper_bound: f64,
}

/// Parses one `estimates.json` file.
///
/// Falls back to the median block when the mean is absent. The standard
/// deviation comes from Criterion's `std_dev` block; when that is missing it
/// falls back to the chosen block's standard error.
#[must_use]
pub fn parse_estimates(json: &str) -> Option<CriterionEstimate> {
    let estimates: Estimates = serde_json::from_str(json).ok()?;
    let standard_deviation = estimates.std_dev.as_ref().map(|block| block.point_estimate);
    let block = estimates.mean.or(estimates.median)?;
    Some(CriterionEstimate {
        point_estimate: block.point_estimate,
        lower_bound: block.confidence_interval.lower_bound,
        upper_bound: block.confidence_interval.upper_bound,
        standard_deviation: standard_deviation.unwrap_or(block.standard_error),
    })
}

/// Walks `target/criterion` and returns `benchmark path → estimate` for
/// every benchmark with parseable estimates. Returns an empty map when
/// the directory does not exist — callers treat that as "Criterion data
/// missing" (spec §74).
#[must_use]
pub fn collect_estimates(criterion_dir: &Path) -> BTreeMap<String, CriterionEstimate> {
    let mut found = BTreeMap::new();
    collect_into(criterion_dir, criterion_dir, &mut found);
    found
}

/// Recursive walk: benchmarks can sit at any depth (`<group>/<bench>/new`),
/// so every directory is visited except Criterion's own result folders.
fn collect_into(criterion_dir: &Path, dir: &Path, found: &mut BTreeMap<String, CriterionEstimate>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        match path.file_name().and_then(|name| name.to_str()) {
            Some("new") => {
                // `dir` is the benchmark directory that owns this `new` folder.
                let Ok(json) = std::fs::read_to_string(path.join("estimates.json")) else {
                    continue;
                };
                if let Some(estimate) = parse_estimates(&json) {
                    if let Ok(relative) = dir.strip_prefix(criterion_dir) {
                        found.insert(benchmark_name(relative), estimate);
                    }
                }
            }
            Some("base" | "change" | "report") => {}
            _ => collect_into(criterion_dir, &path, found),
        }
    }
}

/// Benchmark name = relative path segments joined with `::`.
fn benchmark_name(relative: &Path) -> String {
    relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("::")
}

/// Converts collected estimates into model measurements in seconds
/// (spec §24). Criterion's own confidence intervals are statistical
/// evidence; the profiler marks them accordingly (spec §75).
#[must_use]
pub fn to_measurements(
    estimates: &BTreeMap<String, CriterionEstimate>,
    source: &MeasurementSource,
    now: UnixSeconds,
) -> Vec<Measurement> {
    const NANOS_PER_SECOND: f64 = 1_000_000_000.0;
    estimates
        .iter()
        .map(|(name, estimate)| {
            let mut measurement = Measurement::runtime(
                format!("{name}_bench_mean"),
                estimate.point_estimate / NANOS_PER_SECOND,
                Unit::Seconds,
                source.clone(),
                now,
            );
            measurement.kind = FindingKind::Statistical;
            measurement.confidence = confidence_for_cv(relative_spread(estimate));
            measurement.workload = Some(name.clone());
            measurement
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape written by Criterion 0.5 to `new/estimates.json` (trimmed to the
    /// blocks that matter; `slope` and `median_abs_dev` are present in real
    /// output and must be ignored).
    const SAMPLE: &str = r#"{
        "mean": {
            "confidence_interval": { "confidence_level": 0.95, "lower_bound": 4.7e6, "upper_bound": 5.1e6 },
            "point_estimate": 4.9e6,
            "standard_error": 1.4e4
        },
        "median": {
            "confidence_interval": { "confidence_level": 0.95, "lower_bound": 4.6e6, "upper_bound": 5.0e6 },
            "point_estimate": 4.8e6,
            "standard_error": 7.0e3
        },
        "median_abs_dev": {
            "confidence_interval": { "confidence_level": 0.95, "lower_bound": 1.0e5, "upper_bound": 1.2e5 },
            "point_estimate": 1.1e5,
            "standard_error": 1.0e3
        },
        "slope": null,
        "std_dev": {
            "confidence_interval": { "confidence_level": 0.95, "lower_bound": 9.0e4, "upper_bound": 1.1e5 },
            "point_estimate": 1.0e5,
            "standard_error": 2.0e3
        }
    }"#;

    #[test]
    fn real_criterion_shape_parses_to_nanosecond_estimate() {
        let estimate = parse_estimates(SAMPLE).expect("parses");
        assert!((estimate.point_estimate - 4.9e6).abs() < 1.0);
        assert!((estimate.lower_bound - 4.7e6).abs() < 1.0);
        assert!((estimate.standard_deviation - 1.0e5).abs() < 1.0);
    }

    #[test]
    fn median_falls_back_when_mean_missing() {
        let json = r#"{ "median": { "confidence_interval": { "confidence_level": 0.95, "lower_bound": 1.0, "upper_bound": 2.0 }, "point_estimate": 1.5, "standard_error": 0.1 } }"#;
        let estimate = parse_estimates(json).expect("median fallback");
        assert!((estimate.point_estimate - 1.5).abs() < f64::EPSILON);
        assert!(
            (estimate.standard_deviation - 0.1).abs() < f64::EPSILON,
            "standard error used when std_dev is absent"
        );
    }

    #[test]
    fn criterion_file_on_disk_is_collected_by_directory_walk() {
        let root = std::env::temp_dir().join(format!("tpt-criterion-{}", std::process::id()));
        let estimates = root.join("decode").join("frame").join("new");
        std::fs::create_dir_all(&estimates).expect("mkdir");
        std::fs::write(estimates.join("estimates.json"), SAMPLE).expect("write");
        let collected = collect_estimates(&root);
        std::fs::remove_dir_all(&root).ok();
        assert_eq!(collected.len(), 1);
        assert!(collected.contains_key("decode::frame"), "{collected:?}");
    }

    #[test]
    fn invalid_json_is_none_not_a_crash() {
        assert!(parse_estimates("{ not json").is_none());
    }

    #[test]
    fn measurements_are_in_seconds_and_marked_statistical() {
        let mut estimates = BTreeMap::new();
        estimates.insert(
            String::from("decode::frame"),
            CriterionEstimate {
                point_estimate: 4.9e9,
                lower_bound: 4.7e9,
                upper_bound: 5.1e9,
                standard_deviation: 1.0e8,
            },
        );
        let measurements = to_measurements(&estimates, &MeasurementSource::Criterion, 0);
        assert_eq!(measurements.len(), 1);
        assert_eq!(measurements[0].metric, "decode::frame_bench_mean");
        assert!((measurements[0].value - 4.9).abs() < f64::EPSILON, "ns → s");
        assert_eq!(measurements[0].unit, Unit::Seconds);
        assert_eq!(measurements[0].kind, FindingKind::Statistical);
        // Half-width (5.1e6 - 4.7e6) / 2 / 4.9e6 ≈ 0.041: Medium (≤ 0.05).
        assert_eq!(
            measurements[0].confidence,
            tpt_profiler_model::Confidence::Medium
        );
    }
}
