//! Persistent opportunity tracking (spec §38, §52): stable IDs, lifecycle
//! state, and the false-positive decisions a user makes (dismiss, accept,
//! snooze), kept across runs.
//!
//! Layout:
//!
//! ```text
//! .tpt-profiler/
//! └── opportunities.json   (SchemaEnvelope<Tracking>)
//! ```
//!
//! What is recorded and when:
//!
//! * `baseline` observes the current opportunities, so later runs can tell
//!   what is *new* compared with the reference branch;
//! * `dismiss`, `accept`, `snooze` and `reopen` record a decision.
//!
//! `ci` only reads this file. A pull request therefore never changes the
//! record it is compared against.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tpt_profiler_model::error::{Error, SerializationError};
use tpt_profiler_model::{LifecycleState, Opportunity, OpportunityId, SchemaEnvelope, UnixSeconds};

use crate::baseline::{Assessment, CompareReport, MetricComparison, STATE_DIR};

/// File name of the tracking store under [`STATE_DIR`].
pub const TRACKING_FILE: &str = "opportunities.json";

/// What the record knows about one opportunity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Lifecycle state, including any rejection reason (spec §38).
    pub state: LifecycleState,
    /// When it was first observed.
    pub first_seen: UnixSeconds,
    /// When it was last observed.
    pub last_seen: UnixSeconds,
    /// Hidden from reports until this time, when snoozed.
    pub snoozed_until: Option<UnixSeconds>,
    /// Hidden from reports without a judgement about whether it is real
    /// (`ignore`). Absent in stores written before this field existed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub ignored: bool,
    /// Metrics this opportunity's measured evidence refers to. `compare`
    /// uses them to validate or regress optimised work (spec §37, §38).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<String>,
    /// Category name, kept so decisions can be summarised per category
    /// (spec §52, §80). Absent in stores written before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

// serde's `skip_serializing_if` passes the field by reference.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(value: &bool) -> bool {
    !*value
}

/// The tracking store: every opportunity observed, keyed by stable ID.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Tracking {
    /// Entries keyed by opportunity ID. A `BTreeMap` keeps the file in a
    /// stable order, so it diffs cleanly in git.
    pub entries: BTreeMap<String, Entry>,
}

/// A decision a user records about one opportunity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The finding is real and will be worked on.
    Accept,
    /// The finding is a false positive or not worth doing.
    Dismiss {
        /// Why (kept for the record, spec §38).
        reason: String,
    },
    /// Hide it from reports until the given time.
    Snooze {
        /// Unix time after which it is shown again.
        until: UnixSeconds,
    },
    /// Hide it from reports without judging whether the finding is real.
    Ignore,
    /// The category does not apply to this code. Recorded as a rejection
    /// whose reason says so.
    NotApplicable {
        /// Why the category does not apply.
        reason: String,
    },
    /// Forget the decision and show it again.
    Reopen,
    /// Work on it has started (spec §38: from `Open` or `Accepted`).
    Start,
    /// The change landed and awaits re-measurement (from `InProgress`).
    Optimise,
    /// Re-measurement confirmed the improvement (from `Optimised`).
    Validate,
    /// Re-measurement showed it got worse (from `Optimised`, spec §37).
    Regress,
    /// Another recorded opportunity replaces this one (from any state).
    Supersede {
        /// The opportunity that replaces this one.
        by: OpportunityId,
    },
}

/// Short name of a lifecycle state for error messages.
#[must_use]
pub fn state_name(state: &LifecycleState) -> &'static str {
    match state {
        LifecycleState::Open => "open",
        LifecycleState::Accepted => "accepted",
        LifecycleState::Rejected { .. } => "rejected",
        LifecycleState::InProgress => "in progress",
        LifecycleState::Optimised => "optimised",
        LifecycleState::Validated => "validated",
        LifecycleState::Regressed => "regressed",
        LifecycleState::Superseded { .. } => "superseded",
    }
}

/// Per-category decisions and outcomes (see [`Tracking::category_summary`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CategorySummary {
    /// Category name.
    pub category: String,
    /// Opportunities recorded in this category.
    pub seen: usize,
    /// Rejected (dismissed or not applicable).
    pub dismissed: usize,
    /// Accepted, or in progress.
    pub accepted: usize,
    /// Optimised and awaiting measurement.
    pub optimised: usize,
    /// Optimised work that a comparison validated.
    pub validated: usize,
    /// Optimised work that a comparison found worse.
    pub regressed: usize,
    /// Dismissed as a share of decided opportunities, 0–1.
    pub dismissal_rate: Option<f64>,
    /// Validated as a share of judged optimised work, 0–1 (spec §80).
    pub success_rate: Option<f64>,
}

/// `numerator / denominator` as a fraction, or `None` when there is nothing
/// to divide by.
fn ratio(numerator: usize, denominator: usize) -> Option<f64> {
    #[allow(clippy::cast_precision_loss)]
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

/// Metric names the opportunity's measured evidence refers to.
fn measured_metrics(opportunity: &Opportunity) -> Vec<String> {
    opportunity
        .evidence
        .iter()
        .filter_map(|evidence| evidence.measurement.as_ref())
        .map(|measurement| measurement.metric.clone())
        .collect()
}

/// Path of the tracking store under a workspace root.
#[must_use]
pub fn tracking_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(STATE_DIR).join(TRACKING_FILE)
}

/// Loads the tracking store; `None` when nothing has been recorded yet.
///
/// # Errors
/// Fails on unreadable files, corrupt JSON, or a schema version this build
/// cannot interpret.
pub fn load(workspace_root: &Path) -> Result<Option<Tracking>, Error> {
    let path = tracking_path(workspace_root);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|error| {
        Error::Serialization(SerializationError::DecodeFailed {
            message: format!("could not read {}: {error}", path.display()),
        })
    })?;
    let envelope: SchemaEnvelope<Tracking> = serde_json::from_str(&text).map_err(|error| {
        Error::Serialization(SerializationError::DecodeFailed {
            message: format!("{} is corrupt: {error}", path.display()),
        })
    })?;
    envelope.validate().map_err(Error::Serialization)?;
    Ok(Some(envelope.data))
}

/// Writes the tracking store.
///
/// # Errors
/// Fails on filesystem errors or when the store cannot be serialised.
pub fn save(workspace_root: &Path, tracking: &Tracking) -> Result<PathBuf, Error> {
    let path = tracking_path(workspace_root);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| {
            Error::Serialization(SerializationError::EncodeFailed {
                message: format!("could not create {}: {error}", dir.display()),
            })
        })?;
    }
    let envelope = SchemaEnvelope::new(
        format!("tpt-profiler {}", tpt_profiler_model::version()),
        tracking,
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

impl Tracking {
    /// Records that these opportunities were seen at `now`. Unknown IDs get
    /// a new entry in the `Open` state; known IDs keep their decision.
    pub fn observe(&mut self, opportunities: &[Opportunity], now: UnixSeconds) {
        for opportunity in opportunities {
            let metrics = measured_metrics(opportunity);
            let entry = self
                .entries
                .entry(opportunity.id.as_str().to_owned())
                .and_modify(|entry| entry.last_seen = now)
                .or_insert_with(|| Entry {
                    state: LifecycleState::initial(),
                    first_seen: now,
                    last_seen: now,
                    snoozed_until: None,
                    ignored: false,
                    metrics: Vec::new(),
                    category: None,
                });
            // Keep the metrics of every run that observed it, so a later
            // comparison can still judge the change.
            for metric in metrics {
                if !entry.metrics.contains(&metric) {
                    entry.metrics.push(metric);
                }
            }
            entry.metrics.sort();
            entry
                .category
                .get_or_insert_with(|| opportunity.category.to_string());
        }
    }

    /// Moves optimised opportunities to `Validated` or `Regressed` from a
    /// comparison (spec §37, §38). Only [`LifecycleState::Optimised`] work
    /// is judged, and only through the metrics its evidence measured:
    ///
    /// * any linked metric regressed → `Regressed`;
    /// * otherwise any linked metric improved → `Validated`;
    /// * otherwise (unchanged, noisy, or not compared) → left `Optimised`.
    ///
    /// Returns the IDs whose state changed, with their new state.
    pub fn apply_comparison(
        &mut self,
        comparison: &CompareReport,
    ) -> Vec<(String, LifecycleState)> {
        let mut changed = Vec::new();
        for (id, entry) in &mut self.entries {
            if !matches!(entry.state, LifecycleState::Optimised) || entry.metrics.is_empty() {
                continue;
            }
            let linked: Vec<&MetricComparison> = comparison
                .metrics
                .iter()
                .filter(|metric| entry.metrics.contains(&metric.metric))
                .collect();
            let verdict = if linked
                .iter()
                .any(|metric| metric.assessment == Assessment::Regressed)
            {
                Some(LifecycleState::Regressed)
            } else if linked
                .iter()
                .any(|metric| metric.assessment == Assessment::Improved)
            {
                Some(LifecycleState::Validated)
            } else {
                None
            };
            if let Some(state) = verdict {
                entry.state = state.clone();
                changed.push((id.clone(), state));
            }
        }
        changed
    }

    /// Copies the recorded lifecycle state onto each opportunity, so
    /// reports show decisions made in earlier runs.
    pub fn apply(&self, opportunities: &mut [Opportunity]) {
        for opportunity in opportunities {
            if let Some(entry) = self.entries.get(opportunity.id.as_str()) {
                opportunity.state = entry.state.clone();
            }
        }
    }

    /// Whether an opportunity should be hidden from reports at `now`:
    /// rejected, ignored, or snoozed and not yet expired.
    #[must_use]
    pub fn is_suppressed(&self, id: &str, now: UnixSeconds) -> bool {
        self.entries.get(id).is_some_and(|entry| {
            matches!(entry.state, LifecycleState::Rejected { .. })
                || entry.ignored
                || entry.snoozed_until.is_some_and(|until| until > now)
        })
    }

    /// Opportunities in `current` that were never recorded: the ones that
    /// are *new* since the reference was observed.
    #[must_use]
    pub fn new_opportunities<'a>(&self, current: &'a [Opportunity]) -> Vec<&'a Opportunity> {
        current
            .iter()
            .filter(|opportunity| !self.entries.contains_key(opportunity.id.as_str()))
            .collect()
    }

    /// Decisions and outcomes grouped by category (spec §52, §80): how many
    /// opportunities were seen, how many were dismissed, and how many
    /// optimised changes validated or regressed. Rates are `None` when
    /// there is nothing to divide by.
    #[must_use]
    pub fn category_summary(&self) -> Vec<CategorySummary> {
        let mut by_category: BTreeMap<String, CategorySummary> = BTreeMap::new();
        for entry in self.entries.values() {
            let name = entry
                .category
                .clone()
                .unwrap_or_else(|| String::from("unknown"));
            let summary = by_category
                .entry(name.clone())
                .or_insert_with(|| CategorySummary {
                    category: name,
                    ..CategorySummary::default()
                });
            summary.seen += 1;
            match entry.state {
                LifecycleState::Rejected { .. } => summary.dismissed += 1,
                LifecycleState::Accepted | LifecycleState::InProgress => summary.accepted += 1,
                LifecycleState::Optimised => summary.optimised += 1,
                LifecycleState::Validated => summary.validated += 1,
                LifecycleState::Regressed => summary.regressed += 1,
                LifecycleState::Open | LifecycleState::Superseded { .. } => {}
            }
        }
        by_category
            .into_values()
            .map(|mut summary| {
                let decided =
                    summary.dismissed + summary.accepted + summary.validated + summary.regressed;
                summary.dismissal_rate = ratio(summary.dismissed, decided);
                summary.success_rate =
                    ratio(summary.validated, summary.validated + summary.regressed);
                summary
            })
            .collect()
    }

    /// Resolves an ID or unique prefix against the recorded entries.
    ///
    /// # Errors
    /// Fails when nothing matches or the prefix is ambiguous.
    pub fn resolve(&self, prefix: &str) -> Result<String, String> {
        let matches: Vec<&String> = self
            .entries
            .keys()
            .filter(|id| id.starts_with(prefix))
            .collect();
        match matches.as_slice() {
            [] => Err(format!(
                "no opportunity with id starting `{prefix}` in the record; run `baseline` or `static` first"
            )),
            [only] => Ok((*only).clone()),
            many => Err(format!(
                "`{prefix}` matches {} opportunities ({}); use a longer prefix",
                many.len(),
                many.iter()
                    .map(|id| id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    /// Records a decision for an entry that must already exist.
    ///
    /// # Errors
    /// Fails when `id` is not recorded.
    pub fn decide(&mut self, id: &str, decision: Decision, now: UnixSeconds) -> Result<(), String> {
        if let Decision::Supersede { by } = &decision {
            if by.as_str() == id {
                return Err(format!("`{id}` cannot supersede itself"));
            }
            if !self.entries.contains_key(by.as_str()) {
                return Err(format!("`{by}` is not recorded"));
            }
        }
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| format!("opportunity `{id}` is not recorded"))?;
        match decision {
            Decision::Accept => {
                if matches!(
                    entry.state,
                    LifecycleState::InProgress
                        | LifecycleState::Optimised
                        | LifecycleState::Validated
                        | LifecycleState::Regressed
                        | LifecycleState::Superseded { .. }
                ) {
                    return Err(format!(
                        "cannot accept `{id}`: it is {}; reopen it first",
                        state_name(&entry.state)
                    ));
                }
                entry.state = LifecycleState::Accepted;
                entry.snoozed_until = None;
                entry.ignored = false;
            }
            Decision::Start => {
                if !matches!(entry.state, LifecycleState::Open | LifecycleState::Accepted) {
                    return Err(format!(
                        "cannot start `{id}`: it is {}",
                        state_name(&entry.state)
                    ));
                }
                entry.state = LifecycleState::InProgress;
            }
            Decision::Optimise => {
                if !matches!(entry.state, LifecycleState::InProgress) {
                    return Err(format!(
                        "cannot mark `{id}` optimised: it is {}; start it first",
                        state_name(&entry.state)
                    ));
                }
                entry.state = LifecycleState::Optimised;
            }
            Decision::Validate => {
                if !matches!(entry.state, LifecycleState::Optimised) {
                    return Err(format!(
                        "cannot validate `{id}`: it is {}; optimise it first",
                        state_name(&entry.state)
                    ));
                }
                entry.state = LifecycleState::Validated;
            }
            Decision::Regress => {
                if !matches!(entry.state, LifecycleState::Optimised) {
                    return Err(format!(
                        "cannot mark `{id}` regressed: it is {}; only optimised work can regress",
                        state_name(&entry.state)
                    ));
                }
                entry.state = LifecycleState::Regressed;
            }
            Decision::Supersede { by } => {
                if matches!(entry.state, LifecycleState::Superseded { .. }) {
                    return Err(format!("`{id}` is already superseded"));
                }
                entry.state = LifecycleState::Superseded { by };
                entry.snoozed_until = None;
            }
            Decision::Dismiss { reason } => {
                entry.state = LifecycleState::Rejected { reason };
                entry.snoozed_until = None;
                entry.ignored = false;
            }
            Decision::Snooze { until } => {
                entry.snoozed_until = Some(until.max(now));
            }
            Decision::Ignore => {
                entry.ignored = true;
            }
            Decision::NotApplicable { reason } => {
                entry.state = LifecycleState::Rejected {
                    reason: format!("not applicable: {reason}"),
                };
                entry.snoozed_until = None;
                entry.ignored = false;
            }
            Decision::Reopen => {
                entry.state = LifecycleState::initial();
                entry.snoozed_until = None;
                entry.ignored = false;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_profiler_model::{
        Category, Confidence, Effort, Evidence, EvidenceKind, FindingKind, Generality, Impact,
        Interpretation, Measurement, MeasurementSource, OpportunityId, Priority, Recommendation,
        Risk, RiskDimensions, RiskScore, Unit,
    };

    fn opportunity(description: &str) -> Opportunity {
        Opportunity {
            id: OpportunityId::generate("SIMD", "mdct::transform", description),
            repository: String::from("repo"),
            component: String::from("mdct::transform"),
            category: Category::Simd,
            description: description.to_owned(),
            evidence: Vec::new(),
            impact: Impact::new(9).expect("impact"),
            confidence: Confidence::High,
            effort: Effort::Medium,
            risk: Risk {
                overall: RiskScore::new(3).expect("risk"),
                dimensions: RiskDimensions::default(),
            },
            generality: Generality::new(8).expect("generality"),
            priority: Priority::new(85).ok(),
            interpretation: Interpretation {
                statement: String::from("hot kernel"),
                confidence: Confidence::High,
            },
            recommendation: Recommendation {
                action: String::from("vectorise"),
                rationale: String::from("independent arithmetic"),
            },
            state: LifecycleState::Open,
            kind: FindingKind::Deterministic,
            relationships: Vec::new(),
        }
    }

    #[test]
    fn observe_records_new_ids_once_and_refreshes_last_seen() {
        let mut tracking = Tracking::default();
        let opportunities = [opportunity("vectorisable loop")];
        tracking.observe(&opportunities, 100);
        tracking.observe(&opportunities, 200);
        let entry = tracking.entries.values().next().expect("one entry");
        assert_eq!(tracking.entries.len(), 1);
        assert_eq!(entry.first_seen, 100);
        assert_eq!(entry.last_seen, 200);
    }

    fn measured_opportunity(description: &str, metric: &str) -> Opportunity {
        let mut opportunity = opportunity(description);
        opportunity.evidence.push(Evidence::measured(
            EvidenceKind::BenchmarkRegression,
            "measured",
            Measurement::runtime(
                metric,
                1.0,
                Unit::Seconds,
                MeasurementSource::Criterion,
                100,
            ),
        ));
        opportunity
    }

    fn comparison(metric: &str, assessment: Assessment) -> CompareReport {
        CompareReport {
            metrics: vec![MetricComparison {
                metric: metric.to_owned(),
                baseline: 1.0,
                current: 0.8,
                change: -0.2,
                lower_is_better: true,
                assessment,
                severity: None,
                confidence: Confidence::High,
            }],
            environment_warning: None,
        }
    }

    fn optimised(description: &str, metric: &str) -> (Tracking, String) {
        let mut tracking = Tracking::default();
        let opportunities = [measured_opportunity(description, metric)];
        tracking.observe(&opportunities, 100);
        let id = opportunities[0].id.as_str().to_owned();
        tracking.decide(&id, Decision::Start, 100).expect("start");
        tracking
            .decide(&id, Decision::Optimise, 100)
            .expect("optimise");
        (tracking, id)
    }

    #[test]
    fn improved_linked_metric_validates_optimised_work() {
        let (mut tracking, id) = optimised("vectorisable loop", "bench.kernel");
        let unrelated = tracking.apply_comparison(&comparison("bench.other", Assessment::Improved));
        assert_eq!(unrelated.len(), 0);
        assert_eq!(tracking.entries[&id].state, LifecycleState::Optimised);

        let changed = tracking.apply_comparison(&comparison("bench.kernel", Assessment::Improved));
        assert_eq!(changed, vec![(id.clone(), LifecycleState::Validated)]);
    }

    #[test]
    fn regressed_linked_metric_regresses_optimised_work() {
        let (mut tracking, id) = optimised("vectorisable loop", "bench.kernel");
        let changed = tracking.apply_comparison(&comparison("bench.kernel", Assessment::Regressed));
        assert_eq!(changed, vec![(id.clone(), LifecycleState::Regressed)]);
    }

    #[test]
    fn noisy_or_unchanged_metrics_leave_the_state_alone() {
        let (mut tracking, id) = optimised("vectorisable loop", "bench.kernel");
        assert_eq!(
            tracking
                .apply_comparison(&comparison("bench.kernel", Assessment::Noisy))
                .len(),
            0
        );
        assert_eq!(
            tracking
                .apply_comparison(&comparison("bench.kernel", Assessment::Unchanged))
                .len(),
            0
        );
        assert_eq!(tracking.entries[&id].state, LifecycleState::Optimised);
    }

    #[test]
    fn category_summary_groups_decisions_and_outcomes() {
        let mut tracking = Tracking::default();
        let opportunities = [
            opportunity("first"),
            opportunity("second"),
            opportunity("third"),
        ];
        tracking.observe(&opportunities, 100);
        let ids: Vec<String> = opportunities
            .iter()
            .map(|o| o.id.as_str().to_owned())
            .collect();
        tracking
            .decide(
                &ids[0],
                Decision::Dismiss {
                    reason: String::from("false positive"),
                },
                100,
            )
            .expect("dismiss");
        let summary = tracking.category_summary();
        assert_eq!(summary.len(), 1);
        assert_eq!(summary[0].category, "SIMD");
        assert_eq!(summary[0].seen, 3);
        assert_eq!(summary[0].dismissed, 1);
        assert_eq!(summary[0].dismissal_rate, Some(1.0));
        assert_eq!(summary[0].success_rate, None);
    }

    #[test]
    fn only_optimised_work_is_judged() {
        let mut tracking = Tracking::default();
        let opportunities = [measured_opportunity("vectorisable loop", "bench.kernel")];
        tracking.observe(&opportunities, 100);
        let changed = tracking.apply_comparison(&comparison("bench.kernel", Assessment::Improved));
        assert_eq!(changed.len(), 0);
    }

    #[test]
    fn dismissed_opportunities_are_suppressed_and_keep_their_reason() {
        let mut tracking = Tracking::default();
        let opportunities = [opportunity("vectorisable loop")];
        tracking.observe(&opportunities, 100);
        let id = opportunities[0].id.as_str().to_owned();
        tracking
            .decide(
                &id,
                Decision::Dismiss {
                    reason: String::from("already optimised"),
                },
                150,
            )
            .expect("decide");
        assert!(tracking.is_suppressed(&id, 150));
        assert_eq!(
            tracking.entries[&id].state,
            LifecycleState::Rejected {
                reason: String::from("already optimised")
            }
        );
    }

    #[test]
    fn ignored_opportunities_are_hidden_without_a_rejection() {
        let mut tracking = Tracking::default();
        let opportunities = [opportunity("vectorisable loop")];
        tracking.observe(&opportunities, 100);
        let id = opportunities[0].id.as_str().to_owned();
        tracking.decide(&id, Decision::Ignore, 150).expect("decide");
        assert!(tracking.is_suppressed(&id, 150));
        assert_eq!(tracking.entries[&id].state, LifecycleState::Open);

        tracking.decide(&id, Decision::Reopen, 200).expect("decide");
        assert!(!tracking.is_suppressed(&id, 200));
    }

    #[test]
    fn not_applicable_is_a_rejection_that_says_why() {
        let mut tracking = Tracking::default();
        let opportunities = [opportunity("vectorisable loop")];
        tracking.observe(&opportunities, 100);
        let id = opportunities[0].id.as_str().to_owned();
        tracking
            .decide(
                &id,
                Decision::NotApplicable {
                    reason: String::from("no numeric kernels here"),
                },
                150,
            )
            .expect("decide");
        assert!(tracking.is_suppressed(&id, 150));
        assert_eq!(
            tracking.entries[&id].state,
            LifecycleState::Rejected {
                reason: String::from("not applicable: no numeric kernels here")
            }
        );
    }

    #[test]
    fn progress_follows_start_optimise_validate() {
        let mut tracking = Tracking::default();
        let opportunities = [opportunity("vectorisable loop")];
        tracking.observe(&opportunities, 100);
        let id = opportunities[0].id.as_str().to_owned();
        tracking.decide(&id, Decision::Start, 110).expect("start");
        assert_eq!(tracking.entries[&id].state, LifecycleState::InProgress);
        tracking
            .decide(&id, Decision::Optimise, 120)
            .expect("optimise");
        assert_eq!(tracking.entries[&id].state, LifecycleState::Optimised);
        tracking
            .decide(&id, Decision::Validate, 130)
            .expect("validate");
        assert_eq!(tracking.entries[&id].state, LifecycleState::Validated);
    }

    #[test]
    fn out_of_order_transitions_are_refused_and_change_nothing() {
        let mut tracking = Tracking::default();
        let opportunities = [opportunity("vectorisable loop")];
        tracking.observe(&opportunities, 100);
        let id = opportunities[0].id.as_str().to_owned();
        assert!(tracking.decide(&id, Decision::Optimise, 110).is_err());
        assert!(tracking.decide(&id, Decision::Validate, 110).is_err());
        assert!(tracking.decide(&id, Decision::Regress, 110).is_err());
        assert_eq!(tracking.entries[&id].state, LifecycleState::Open);
    }

    #[test]
    fn regression_is_only_recorded_after_optimisation() {
        let mut tracking = Tracking::default();
        let opportunities = [opportunity("vectorisable loop")];
        tracking.observe(&opportunities, 100);
        let id = opportunities[0].id.as_str().to_owned();
        tracking.decide(&id, Decision::Start, 110).expect("start");
        tracking
            .decide(&id, Decision::Optimise, 120)
            .expect("optimise");
        tracking
            .decide(&id, Decision::Regress, 130)
            .expect("regress");
        assert_eq!(tracking.entries[&id].state, LifecycleState::Regressed);
    }

    #[test]
    fn accept_does_not_wipe_work_in_progress() {
        let mut tracking = Tracking::default();
        let opportunities = [opportunity("vectorisable loop")];
        tracking.observe(&opportunities, 100);
        let id = opportunities[0].id.as_str().to_owned();
        tracking.decide(&id, Decision::Start, 110).expect("start");
        let error = tracking
            .decide(&id, Decision::Accept, 120)
            .expect_err("refused");
        assert!(error.contains("in progress"), "{error}");
        assert_eq!(tracking.entries[&id].state, LifecycleState::InProgress);
    }

    #[test]
    fn supersede_needs_a_recorded_different_opportunity() {
        let mut tracking = Tracking::default();
        let opportunities = [opportunity("vectorisable loop"), opportunity("sort loop")];
        tracking.observe(&opportunities, 100);
        let first = opportunities[0].id.as_str().to_owned();
        let second = opportunities[1].id.as_str().to_owned();
        assert!(
            tracking
                .decide(
                    &first,
                    Decision::Supersede {
                        by: OpportunityId::from_stored(&first),
                    },
                    110,
                )
                .is_err()
        );
        assert!(
            tracking
                .decide(
                    &first,
                    Decision::Supersede {
                        by: OpportunityId::from_stored("SIMD-00000"),
                    },
                    110,
                )
                .is_err()
        );
        tracking
            .decide(
                &first,
                Decision::Supersede {
                    by: OpportunityId::from_stored(&second),
                },
                120,
            )
            .expect("supersede");
        assert_eq!(
            tracking.entries[&first].state,
            LifecycleState::Superseded {
                by: OpportunityId::from_stored(&second)
            }
        );
    }

    #[test]
    fn stores_without_the_ignored_field_still_load() {
        let json = r#"{"state":"open","first_seen":1,"last_seen":2,"snoozed_until":null}"#;
        let entry: Entry = serde_json::from_str(json).expect("older entry");
        assert!(!entry.ignored);
    }

    #[test]
    fn snooze_hides_until_it_expires() {
        let mut tracking = Tracking::default();
        let opportunities = [opportunity("vectorisable loop")];
        tracking.observe(&opportunities, 100);
        let id = opportunities[0].id.as_str().to_owned();
        tracking
            .decide(&id, Decision::Snooze { until: 500 }, 100)
            .expect("decide");
        assert!(tracking.is_suppressed(&id, 499));
        assert!(!tracking.is_suppressed(&id, 500));
    }

    #[test]
    fn reopen_clears_dismissal_and_snooze() {
        let mut tracking = Tracking::default();
        let opportunities = [opportunity("vectorisable loop")];
        tracking.observe(&opportunities, 100);
        let id = opportunities[0].id.as_str().to_owned();
        tracking
            .decide(
                &id,
                Decision::Dismiss {
                    reason: String::from("noise"),
                },
                100,
            )
            .expect("dismiss");
        tracking.decide(&id, Decision::Reopen, 110).expect("reopen");
        assert!(!tracking.is_suppressed(&id, 110));
        assert_eq!(tracking.entries[&id].state, LifecycleState::Open);
    }

    #[test]
    fn new_opportunities_are_those_never_recorded() {
        let mut tracking = Tracking::default();
        let known = [opportunity("known loop")];
        tracking.observe(&known, 100);
        let current = [opportunity("known loop"), opportunity("fresh loop")];
        let new = tracking.new_opportunities(&current);
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].description, "fresh loop");
    }

    #[test]
    fn apply_copies_recorded_state_onto_opportunities() {
        let mut tracking = Tracking::default();
        let mut opportunities = [opportunity("vectorisable loop")];
        tracking.observe(&opportunities, 100);
        let id = opportunities[0].id.as_str().to_owned();
        tracking.decide(&id, Decision::Accept, 100).expect("accept");
        tracking.apply(&mut opportunities);
        assert_eq!(opportunities[0].state, LifecycleState::Accepted);
    }

    #[test]
    fn resolve_accepts_unique_prefix_and_rejects_ambiguity() {
        let mut tracking = Tracking::default();
        let opportunities = [opportunity("first loop"), opportunity("second loop")];
        tracking.observe(&opportunities, 100);
        let full = opportunities[0].id.as_str().to_owned();
        assert_eq!(tracking.resolve(&full).expect("exact"), full);
        assert!(tracking.resolve("SIMD-").is_err(), "ambiguous prefix");
        assert!(tracking.resolve("NOPE").is_err(), "no match");
    }

    #[test]
    fn decide_on_unknown_id_is_an_error() {
        let mut tracking = Tracking::default();
        assert!(tracking.decide("SIMD-00000", Decision::Accept, 0).is_err());
    }

    #[test]
    fn store_round_trips_through_the_envelope() {
        let directory = std::env::temp_dir().join(format!("tpt-tracking-{}", std::process::id()));
        let mut tracking = Tracking::default();
        tracking.observe(&[opportunity("round trip")], 42);
        save(&directory, &tracking).expect("save");
        let loaded = load(&directory).expect("load").expect("present");
        std::fs::remove_dir_all(&directory).ok();
        assert_eq!(loaded, tracking);
    }

    #[test]
    fn missing_store_is_a_state_not_an_error() {
        let directory =
            std::env::temp_dir().join(format!("tpt-tracking-none-{}", std::process::id()));
        assert_eq!(load(&directory).expect("no error"), None);
    }
}
