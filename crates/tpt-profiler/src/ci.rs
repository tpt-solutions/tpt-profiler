//! CI policy evaluation (spec §39).
//!
//! The comparison itself is noise-aware and threshold-based
//! ([`crate::baseline::compare`]); this module only decides how each
//! verdict is reported: fail the run, warn, or note. Keeping the policy
//! pure makes it testable without running any workload.

use crate::baseline::{Assessment, CompareReport};
use crate::config::CiConfig;

/// How serious one policy outcome is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Worth knowing; never affects the exit status.
    Info,
    /// Reported as a warning; never affects the exit status.
    Warn,
    /// Fails the CI run (spec §39).
    Fail,
}

/// One outcome of applying the policy to a comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Severity of this outcome.
    pub severity: Severity,
    /// Human-readable explanation.
    pub message: String,
}

/// Applies the `[ci]` policy to a comparison (spec §39).
///
/// * A regressed metric fails the run when `fail_on_regression` is set,
///   and warns otherwise.
/// * A noisy metric is noted, never judged.
/// * An environment mismatch between baseline and current is a warning.
/// * A comparison with no measurements is noted, so an empty result is
///   never mistaken for a pass.
#[must_use]
pub fn evaluate(comparison: &CompareReport, policy: &CiConfig) -> Vec<Outcome> {
    let mut outcomes = Vec::new();

    if let Some(warning) = &comparison.environment_warning {
        outcomes.push(Outcome {
            severity: Severity::Warn,
            message: format!("environment differs from baseline: {warning}"),
        });
    }
    if comparison.metrics.is_empty() {
        outcomes.push(Outcome {
            severity: Severity::Info,
            message: String::from("no comparable measurements; nothing was judged"),
        });
    }

    for metric in &comparison.metrics {
        match metric.assessment {
            Assessment::Regressed => {
                let severity = if policy.fail_on_regression {
                    Severity::Fail
                } else {
                    Severity::Warn
                };
                outcomes.push(Outcome {
                    severity,
                    message: format!(
                        "{} regressed by {} (baseline {:.4} → current {:.4})",
                        metric.metric,
                        metric.change_percent(),
                        metric.baseline,
                        metric.current,
                    ),
                });
            }
            Assessment::Noisy => outcomes.push(Outcome {
                severity: Severity::Info,
                message: format!("{} is too noisy to judge", metric.metric),
            }),
            Assessment::Improved | Assessment::Unchanged => {}
        }
    }

    outcomes
}

/// Whether any outcome fails the run.
#[must_use]
pub fn failed(outcomes: &[Outcome]) -> bool {
    outcomes
        .iter()
        .any(|outcome| outcome.severity == Severity::Fail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::baseline::MetricComparison;

    fn metric(name: &str, change: f64, assessment: Assessment) -> MetricComparison {
        MetricComparison {
            metric: name.to_owned(),
            baseline: 1.0,
            current: 1.0 + change,
            change,
            lower_is_better: true,
            assessment,
        }
    }

    fn policy(fail_on_regression: bool) -> CiConfig {
        CiConfig {
            enabled: true,
            fail_on_regression,
            runtime_threshold: 0.10,
            binary_threshold: 0.05,
        }
    }

    #[test]
    fn regression_fails_when_policy_says_so() {
        let comparison = CompareReport {
            metrics: vec![metric("runtime.wall_s", 0.25, Assessment::Regressed)],
            environment_warning: None,
        };
        let outcomes = evaluate(&comparison, &policy(true));
        assert!(failed(&outcomes));
        assert!(outcomes[0].message.contains("regressed by +25.0%"));
    }

    #[test]
    fn regression_only_warns_when_failing_is_disabled() {
        let comparison = CompareReport {
            metrics: vec![metric("runtime.wall_s", 0.25, Assessment::Regressed)],
            environment_warning: None,
        };
        let outcomes = evaluate(&comparison, &policy(false));
        assert!(!failed(&outcomes));
        assert_eq!(outcomes[0].severity, Severity::Warn);
    }

    #[test]
    fn improvements_and_unchanged_metrics_are_silent() {
        let comparison = CompareReport {
            metrics: vec![
                metric("runtime.wall_s", -0.30, Assessment::Improved),
                metric("runtime.cpu_s", 0.01, Assessment::Unchanged),
            ],
            environment_warning: None,
        };
        let outcomes = evaluate(&comparison, &policy(true));
        assert!(outcomes.is_empty());
    }

    #[test]
    fn noisy_metrics_are_noted_never_failed() {
        let comparison = CompareReport {
            metrics: vec![metric("bench.mean_s", 0.80, Assessment::Noisy)],
            environment_warning: None,
        };
        let outcomes = evaluate(&comparison, &policy(true));
        assert!(!failed(&outcomes));
        assert_eq!(outcomes[0].severity, Severity::Info);
    }

    #[test]
    fn empty_comparison_is_noted_not_passed_silently() {
        let outcomes = evaluate(&CompareReport::default(), &policy(true));
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].severity, Severity::Info);
        assert!(!failed(&outcomes));
    }

    #[test]
    fn environment_mismatch_is_a_warning() {
        let comparison = CompareReport {
            metrics: Vec::new(),
            environment_warning: Some(String::from("OS differs")),
        };
        let outcomes = evaluate(&comparison, &policy(true));
        assert!(
            outcomes
                .iter()
                .any(|outcome| outcome.severity == Severity::Warn)
        );
    }
}
