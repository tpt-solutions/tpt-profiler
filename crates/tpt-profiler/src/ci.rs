//! CI policy evaluation (spec §39).
//!
//! The comparison itself is noise-aware and threshold-based
//! ([`crate::baseline::compare`]); this module only decides how each
//! verdict is reported: fail the run, warn, or note. Keeping the policy
//! pure makes it testable without running any workload.

use tpt_profiler_model::Opportunity;

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
                // Binary growth is a warning by policy (spec §39); only
                // runtime and benchmark regressions can fail the run.
                let is_size = metric
                    .metric
                    .starts_with(crate::measure::BINARY_METRIC_PREFIX);
                let severity = if policy.fail_on_regression && !is_size {
                    Severity::Fail
                } else {
                    Severity::Warn
                };
                let grade = metric
                    .severity
                    .map_or(String::new(), |grade| format!("{} ", grade.label()));
                outcomes.push(Outcome {
                    severity,
                    message: format!(
                        "{} regressed by {} ({grade}regression, {} confidence; baseline {:.4} → current {:.4})",
                        metric.metric,
                        metric.change_percent(),
                        metric.confidence,
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

/// Warns about opportunities that are new since the reference and whose
/// priority is at or above `threshold` (spec §39). Never fails the run:
/// a new finding is information to act on, not a regression.
#[must_use]
pub fn new_high_priority(new: &[&Opportunity], threshold: u8) -> Vec<Outcome> {
    new.iter()
        .filter(|opportunity| {
            opportunity
                .priority
                .is_some_and(|priority| priority.value() >= threshold)
        })
        .map(|opportunity| Outcome {
            severity: Severity::Warn,
            message: format!(
                "new HIGH-priority opportunity {} ({} in {}, priority {}): {}",
                opportunity.id,
                opportunity.category.as_slug(),
                opportunity.component,
                opportunity
                    .priority
                    .map_or(0, tpt_profiler_model::Priority::value),
                opportunity.description,
            ),
        })
        .collect()
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
    use crate::baseline::{MetricComparison, RegressionSeverity};
    use tpt_profiler_model::Confidence;

    fn metric(name: &str, change: f64, assessment: Assessment) -> MetricComparison {
        MetricComparison {
            metric: name.to_owned(),
            baseline: 1.0,
            current: 1.0 + change,
            change,
            lower_is_better: true,
            assessment,
            severity: (assessment == Assessment::Regressed).then_some(RegressionSeverity::Minor),
            confidence: Confidence::High,
        }
    }

    fn policy(fail_on_regression: bool) -> CiConfig {
        CiConfig {
            enabled: true,
            fail_on_regression,
            runtime_threshold: 0.10,
            binary_threshold: 0.05,
            high_priority: 70,
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
        assert_eq!(outcomes.len(), 0);
    }

    #[test]
    fn new_high_priority_warns_only_at_or_above_the_threshold() {
        let mut high = opportunity_with_priority(85);
        high.description = String::from("hot loop");
        let low = opportunity_with_priority(20);
        let new = [&high, &low];
        let outcomes = new_high_priority(&new, 70);
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].severity, Severity::Warn);
        assert!(outcomes[0].message.contains("new HIGH-priority"));
        assert!(outcomes[0].message.contains("priority 85"));
        assert!(!failed(&outcomes));
    }

    fn opportunity_with_priority(priority: u8) -> Opportunity {
        use tpt_profiler_model::{
            Category, Confidence, Effort, FindingKind, Generality, Impact, Interpretation,
            LifecycleState, OpportunityId, Priority, Recommendation, Risk, RiskDimensions,
            RiskScore,
        };
        Opportunity {
            id: OpportunityId::generate("SIMD", "mdct", &priority.to_string()),
            repository: String::from("repo"),
            component: String::from("mdct"),
            category: Category::Simd,
            description: String::from("vectorisable loop"),
            evidence: Vec::new(),
            impact: Impact::new(9).expect("impact"),
            confidence: Confidence::High,
            effort: Effort::Medium,
            risk: Risk {
                overall: RiskScore::new(3).expect("risk"),
                dimensions: RiskDimensions::default(),
            },
            generality: Generality::new(8).expect("generality"),
            priority: Priority::new(priority).ok(),
            interpretation: Interpretation {
                statement: String::from("hot"),
                confidence: Confidence::High,
            },
            recommendation: Recommendation {
                action: String::from("vectorise"),
                rationale: String::from("arithmetic"),
            },
            state: LifecycleState::Open,
            kind: FindingKind::Deterministic,
            relationships: Vec::new(),
        }
    }

    #[test]
    fn binary_growth_warns_even_when_failing_is_enabled() {
        let comparison = CompareReport {
            metrics: vec![metric(
                "binary:tpt-profiler_bytes",
                0.20,
                Assessment::Regressed,
            )],
            environment_warning: None,
        };
        let outcomes = evaluate(&comparison, &policy(true));
        assert!(!failed(&outcomes));
        assert_eq!(outcomes[0].severity, Severity::Warn);
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
