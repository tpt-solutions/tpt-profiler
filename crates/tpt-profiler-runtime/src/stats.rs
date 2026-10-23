//! Statistical engine (spec §50): summaries of repeated runs with
//! outlier handling and honest confidence.
//!
//! A single benchmark execution is never definitive (spec §50, §75): the
//! runner discards warm-up samples, and the summary maps sample spread to
//! a [`tpt_profiler_model::Confidence`] so downstream scoring can never over-claim.

/// Summary of a set of duration/value samples (spec §50).
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// Number of samples retained (after warm-up removal).
    pub n: usize,
    /// Arithmetic mean.
    pub mean: f64,
    /// Median.
    pub median: f64,
    /// Sample standard deviation.
    pub stddev: f64,
    /// Minimum sample.
    pub min: f64,
    /// Maximum sample.
    pub max: f64,
    /// 95% confidence interval of the mean (normal approximation; the
    /// exact t-distribution needs a table dependency the profiler
    /// avoids, spec §72 — the approximation is documented here and in
    /// reports).
    pub ci95: (f64, f64),
    /// Samples rejected as outliers (1.5 × IQR fences). Kept for
    /// reporting; excluded from the statistics above.
    pub outliers: Vec<f64>,
}

/// Statistical confidence derived from sample spread.
///
/// * a single execution is always `Low` (spec §50),
/// * tight spread (CV ≤ 2%) is `High`,
/// * CV ≤ 5% is `Medium`,
/// * anything noisier stays `Low`.
pub const MAX_CV_HIGH: f64 = 0.02;
/// Coefficient-of-variation ceiling for `Medium` confidence.
pub const MAX_CV_MEDIUM: f64 = 0.05;

impl Summary {
    /// Summarises samples; `None` when empty. Warm-up samples must be
    /// removed by the caller before this point.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "sample counts are far below 2^52"
    )]
    pub fn from_samples(samples: &[f64]) -> Option<Self> {
        if samples.is_empty() {
            return None;
        }
        let mut values = samples.to_vec();
        values.sort_by(f64::total_cmp);
        let n = values.len();
        let median = if n % 2 == 1 {
            values[n / 2]
        } else {
            f64::midpoint(values[n / 2 - 1], values[n / 2])
        };
        let mean = values.iter().sum::<f64>() / n as f64;
        let variance = if n > 1 {
            values
                .iter()
                .map(|value| (value - mean) * (value - mean))
                .sum::<f64>()
                / (n - 1) as f64
        } else {
            0.0
        };
        let stddev = variance.sqrt();

        // Outlier rejection: 1.5 × IQR fences using Tukey's hinges
        // (quartiles as medians of halves — interpolated percentiles are
        // too tight for the small samples typical of benchmarks).
        let q1 = hinge(&values, false);
        let q3 = hinge(&values, true);
        let iqr = q3 - q1;
        let (low, high) = (q1 - 1.5 * iqr, q3 + 1.5 * iqr);
        let kept: Vec<f64> = values
            .iter()
            .copied()
            .filter(|value| *value >= low && *value <= high)
            .collect();
        let outliers: Vec<f64> = values
            .iter()
            .copied()
            .filter(|value| *value < low || *value > high)
            .collect();

        // Re-derive over the retained samples when outliers were cut.
        let (n, mean, stddev, median, min, max) = if kept.is_empty() || kept.len() == values.len() {
            (n, mean, stddev, median, values[0], values[n - 1])
        } else {
            Self::from_samples(&kept).map_or_else(
                || (n, mean, stddev, median, values[0], values[n - 1]),
                |trimmed| {
                    (
                        trimmed.n,
                        trimmed.mean,
                        trimmed.stddev,
                        trimmed.median,
                        trimmed.min,
                        trimmed.max,
                    )
                },
            )
        };

        let standard_error = stddev / (n as f64).sqrt();
        let margin = 1.96 * standard_error;
        let summary = Self {
            n,
            mean,
            median,
            stddev,
            min,
            max,
            ci95: (mean - margin, mean + margin),
            outliers,
        };
        Some(summary)
    }

    /// Coefficient of variation (stddev / mean).
    #[must_use]
    pub fn coefficient_of_variation(&self) -> f64 {
        if self.mean == 0.0 {
            0.0
        } else {
            self.stddev / self.mean
        }
    }

    /// Honest confidence for this summary (spec §50: a single execution
    /// is never definitive).
    #[must_use]
    pub fn confidence(&self) -> tpt_profiler_model::Confidence {
        use tpt_profiler_model::Confidence;
        if self.n < 2 {
            return Confidence::Low;
        }
        confidence_for_cv(self.coefficient_of_variation())
    }
}

/// Confidence implied by a coefficient of variation (spec §50): the tighter
/// the spread of the samples, the more the mean can be trusted. Shared by
/// runtime and bench measurements so both use the same standard.
#[must_use]
pub fn confidence_for_cv(cv: f64) -> tpt_profiler_model::Confidence {
    use tpt_profiler_model::Confidence;
    if cv <= MAX_CV_HIGH {
        Confidence::High
    } else if cv <= MAX_CV_MEDIUM {
        Confidence::Medium
    } else {
        Confidence::Low
    }
}

/// Tukey's hinge: the median of the lower (or upper) half of a sorted
/// slice, excluding the overall median element for odd lengths.
fn hinge(sorted: &[f64], upper: bool) -> f64 {
    debug_assert_ne!(sorted, []);
    let n = sorted.len();
    if n == 1 {
        return sorted[0];
    }
    let middle = n / 2;
    let (start, end) = if upper {
        (n.div_ceil(2), n)
    } else {
        (0, middle)
    };
    let half = &sorted[start..end];
    half[half.len() / 2]
}

#[cfg(test)]
#[allow(clippy::float_cmp, reason = "fixtures are exact by construction")]
mod tests {
    use super::*;

    fn samples(values: &[f64]) -> Summary {
        Summary::from_samples(values).expect("non-empty")
    }

    #[test]
    fn summary_of_stable_samples_is_high_confidence() {
        let stable: Vec<f64> = (0_u32..20)
            .map(|i| 100.0 + f64::from(i % 2) * 0.5)
            .collect();
        let summary = samples(&stable);
        assert_eq!(summary.n, 20);
        assert_eq!(summary.confidence(), tpt_profiler_model::Confidence::High);
        assert!(summary.ci95.0 < summary.mean && summary.mean < summary.ci95.1);
    }

    #[test]
    fn a_single_execution_is_low_confidence() {
        let summary = samples(&[42.0]);
        assert_eq!(summary.n, 1);
        assert_eq!(summary.stddev, 0.0);
        assert_eq!(summary.confidence(), tpt_profiler_model::Confidence::Low);
    }

    #[test]
    fn noisy_samples_reduce_confidence() {
        let noisy: Vec<f64> = [10.0, 90.0, 20.0, 95.0, 15.0, 85.0, 25.0, 90.0, 12.0, 88.0].to_vec();
        let summary = samples(&noisy);
        assert_eq!(summary.confidence(), tpt_profiler_model::Confidence::Low);
    }

    #[test]
    fn outliers_are_detected_and_excluded() {
        let values = [100.0, 100.0, 101.0, 100.0, 99.0, 100.0, 100.0, 500.0];
        let summary = samples(&values);
        assert_eq!(summary.outliers, vec![500.0]);
        assert!(
            (summary.mean - 100.0).abs() < 1.0,
            "mean must exclude the outlier: {}",
            summary.mean
        );
        assert!(summary.max < 500.0, "max is over retained samples only");
    }

    #[test]
    fn median_splits_even_and_odd_counts() {
        assert_eq!(samples(&[3.0, 1.0, 2.0]).median, 2.0);
        assert_eq!(samples(&[4.0, 1.0, 2.0, 3.0]).median, 2.5);
    }

    #[test]
    fn empty_samples_are_no_summary() {
        assert!(Summary::from_samples(&[]).is_none());
    }

    #[test]
    fn hinges_split_the_sorted_samples() {
        let sorted = [10.0, 20.0, 30.0, 40.0];
        assert_eq!(hinge(&sorted, false), 20.0);
        assert_eq!(hinge(&sorted, true), 40.0);
        let odd = [10.0, 20.0, 30.0];
        assert_eq!(hinge(&odd, false), 10.0);
        assert_eq!(hinge(&odd, true), 30.0);
    }
}
