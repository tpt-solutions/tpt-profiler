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
/// fields are ignored by serde).
#[derive(Debug, Deserialize)]
struct Estimates {
    #[serde(default)]
    mean: Option<EstimateBlock>,
    #[serde(default)]
    median: Option<EstimateBlock>,
}

#[derive(Debug, Deserialize)]
struct EstimateBlock {
    #[serde(rename = "confidence_interval")]
    confidence_interval: ConfidenceInterval,
    point_estimate: f64,
    standard_deviation: f64,
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
/// Falls back to the median block when the mean is absent.
#[must_use]
pub fn parse_estimates(json: &str) -> Option<CriterionEstimate> {
    let estimates: Estimates = serde_json::from_str(json).ok()?;
    let block = estimates.mean.or(estimates.median)?;
    Some(CriterionEstimate {
        point_estimate: block.point_estimate,
        lower_bound: block.confidence_interval.lower_bound,
        upper_bound: block.confidence_interval.upper_bound,
        standard_deviation: block.standard_deviation,
    })
}

/// Walks `target/criterion` and returns `benchmark path → estimate` for
/// every benchmark with parseable estimates. Returns an empty map when
/// the directory does not exist — callers treat that as "Criterion data
/// missing" (spec §74).
#[must_use]
pub fn collect_estimates(criterion_dir: &Path) -> BTreeMap<String, CriterionEstimate> {
    let mut found = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(criterion_dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let bench_dir = entry.path();
        let estimates_path = bench_dir.join("new").join("estimates.json");
        let Ok(json) = std::fs::read_to_string(&estimates_path) else {
            continue;
        };
        if let Some(estimate) = parse_estimates(&json) {
            // Benchmark name = relative path segments joined with `::`.
            let name = bench_dir
                .strip_prefix(criterion_dir)
                .unwrap_or(&bench_dir)
                .components()
                .map(|component| component.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("::");
            found.insert(name, estimate);
        }
    }
    found
}

/// Converts collected estimates into model measurements in seconds
/// (spec §24). Criterion's own confidence intervals are statistical
/// evidence; the profiler marks them accordingly (spec §75).
#[must_use]
pub fn to_measurements(
    estimates: &BTreeMap<String, CriterionEstimate>,
    source: MeasurementSource,
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
            measurement.workload = Some(name.clone());
            measurement
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "mean": {
            "confidence_interval": { "confidence_level": 0.95, "lower_bound": 4.7e6, "upper_bound": 5.1e6 },
            "point_estimate": 4.9e6,
            "standard_deviation": 1.0e5
        },
        "median": {
            "confidence_interval": { "confidence_level": 0.95, "lower_bound": 4.6e6, "upper_bound": 5.0e6 },
            "point_estimate": 4.8e6,
            "standard_deviation": 9.0e4
        }
    }"#;

    #[test]
    fn estimates_json_parses_to_nanosecond_estimate() {
        let estimate = parse_estimates(SAMPLE).expect("parses");
        assert!((estimate.point_estimate - 4.9e6).abs() < 1.0);
        assert!((estimate.lower_bound - 4.7e6).abs() < 1.0);
        assert!((estimate.standard_deviation - 1.0e5).abs() < 1.0);
    }

    #[test]
    fn median_falls_back_when_mean_missing() {
        let json = r#"{ "median": { "confidence_interval": { "confidence_level": 0.95, "lower_bound": 1.0, "upper_bound": 2.0 }, "point_estimate": 1.5, "standard_deviation": 0.1 } }"#;
        let estimate = parse_estimates(json).expect("median fallback");
        assert!((estimate.point_estimate - 1.5).abs() < f64::EPSILON);
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
        let measurements = to_measurements(&estimates, MeasurementSource::Criterion, 0);
        assert_eq!(measurements.len(), 1);
        assert_eq!(measurements[0].metric, "decode::frame_bench_mean");
        assert!((measurements[0].value - 4.9).abs() < f64::EPSILON, "ns → s");
        assert_eq!(measurements[0].unit, Unit::Seconds);
        assert_eq!(measurements[0].kind, FindingKind::Statistical);
    }
}
