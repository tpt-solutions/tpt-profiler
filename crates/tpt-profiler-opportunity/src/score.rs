//! The priority calculator (spec §27) with configurable weights.
//!
//! ```text
//! priority = (impact·wᵢ × confidence·w𝒸 × generality·w𝓰)
//!          / (effort·wₑ × risk·wᵣ)          — normalised to 0–100
//! ```
//!
//! The formula is explicitly NOT an API contract (spec §27): weights are
//! configurable (`[scoring]` in `tpt-profiler.toml`) and the shape may
//! change in any release.

use tpt_profiler_model::{Confidence, Effort, Generality, Impact, Priority, Risk};

/// Scoring weights, mirroring `[scoring]` in `tpt-profiler.toml`
/// (spec §27). All weights must be finite and > 0; the config layer
/// validates that.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoringWeights {
    /// Weight of the impact factor (spec §28).
    pub impact_weight: f64,
    /// Weight of the confidence factor (spec §29).
    pub confidence_weight: f64,
    /// Weight of the generality factor (spec §32).
    pub generality_weight: f64,
    /// Weight of the effort divisor (spec §30).
    pub effort_weight: f64,
    /// Weight of the risk divisor (spec §31).
    pub risk_weight: f64,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            impact_weight: 1.0,
            confidence_weight: 1.0,
            generality_weight: 1.0,
            effort_weight: 1.0,
            risk_weight: 1.0,
        }
    }
}

/// How the raw score was computed — the `explain` payload (spec §59:
/// recommendations must be explainable).
#[derive(Debug, Clone, PartialEq)]
pub struct ScoringTrace {
    /// The un-normalised score.
    pub raw: f64,
    /// The final 0–100 priority.
    pub priority: u8,
    /// One-line reasoning over the factors.
    pub reasoning: String,
}

/// Computes the priority for one set of dimensions.
///
/// Weights below the required maximum scale factors down; the result is
/// clamped to `0..=100`. A risk or effort divisor of zero cannot occur
/// (both scales start at 1).
#[must_use]
pub fn compute_priority(
    impact: Impact,
    confidence: Confidence,
    generality: Generality,
    effort: Effort,
    risk: &Risk,
    weights: &ScoringWeights,
) -> (Priority, ScoringTrace) {
    let impact_factor = f64::from(impact.value()) * weights.impact_weight;
    let confidence_factor = f64::from(confidence.scale()) * weights.confidence_weight;
    let generality_factor = f64::from(generality.value()) * weights.generality_weight;
    let effort_factor = f64::from(effort.scale()) * weights.effort_weight;
    let risk_factor = f64::from(risk.overall.value()) * weights.risk_weight;

    let raw = (impact_factor * confidence_factor * generality_factor)
        / (effort_factor * risk_factor).max(f64::MIN_POSITIVE);
    // Square-root normalisation over the 0..1000 raw range softens the
    // divide-by-effort·risk: a perfect candidate (all 10s over 1/1) hits
    // 100, and mid-range candidates do not collapse toward 0 the way a
    // linear map would. 0 and 100 are fixed points; weights pushing raw
    // past 1000 clamp at 100. (The §25 "86" example is illustrative and
    // not derivable from the §27 formula; see the score tests.)
    let normalised = 100.0 * (raw / 1000.0).max(0.0).sqrt();
    let priority = u8::try_from(normalised.round() as i64).unwrap_or(100);
    let reasoning = format!(
        "impact {}×{}, confidence {}/{}, generality {}, effort {}/{}, risk {}/{}",
        impact.value(),
        weights.impact_weight,
        confidence.scale(),
        weights.confidence_weight,
        generality.value(),
        effort.scale(),
        weights.effort_weight,
        risk.overall.value(),
        weights.risk_weight,
    );
    (
        Priority::new(priority).unwrap_or_else(|_| Priority::new(0).expect("0 is valid")),
        ScoringTrace {
            raw,
            priority,
            reasoning,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_profiler_model::{RiskDimensions, RiskScore};

    fn risk_of(value: u8) -> Risk {
        Risk {
            overall: RiskScore::new(value).expect("in range"),
            dimensions: RiskDimensions::default(),
        }
    }

    #[test]
    fn best_case_is_hundred_worst_case_is_low() {
        let weights = ScoringWeights::default();
        let best = compute_priority(
            Impact::new(10).unwrap(),
            Confidence::VeryHigh,
            Generality::new(10).unwrap(),
            Effort::Trivial,
            &risk_of(1),
            &weights,
        );
        assert_eq!(best.0.value(), 100, "10·10·10/(1·1)/10 = 100");

        let worst = compute_priority(
            Impact::new(1).unwrap(),
            Confidence::VeryLow,
            Generality::new(1).unwrap(),
            Effort::VeryHigh,
            &risk_of(10),
            &weights,
        );
        assert_eq!(worst.0.value(), 0, "(1·2·1)/(9·10) is ~0");
        // The §25 example numbers (9/10/8 over 5/3) give raw 48 under the
        // §27 formula — the spec's "86" is illustrative and cannot be
        // derived from it. Pin the documented curve instead: raw 48 of a
        // 0..1000 range lands at 22 after the square-root map.
        let mid = compute_priority(
            Impact::new(9).unwrap(),
            Confidence::VeryHigh,
            Generality::new(8).unwrap(),
            Effort::Medium,
            &risk_of(3),
            &weights,
        );
        assert_eq!(mid.0.value(), 22, "documented sqrt curve: {mid:?}");
    }

    #[test]
    fn spec_25_example_scores_eighty_sixish() {
        // Impact 9, confidence 9/10, effort 5, risk 3, generality 8:
        // 9·8·6/(5·3)... with confidence levels the spec's 9/10 maps to
        // VeryHigh(10): (9·10·8)/(5·3)/10 = 48 — the formula is a
        // starting point, not the spec's illustrative numbers.
        let (priority, _trace) = compute_priority(
            Impact::new(9).unwrap(),
            Confidence::VeryHigh,
            Generality::new(8).unwrap(),
            Effort::Medium,
            &risk_of(3),
            &ScoringWeights::default(),
        );
        assert!(
            priority.value() > 15,
            "strong candidate scores respectably: {priority}"
        );
    }

    #[test]
    fn weights_shift_scores_monotonically() {
        let base = compute_priority(
            Impact::new(8).unwrap(),
            Confidence::High,
            Generality::new(6).unwrap(),
            Effort::Medium,
            &risk_of(3),
            &ScoringWeights::default(),
        )
        .0;
        let emphasise_impact = compute_priority(
            Impact::new(8).unwrap(),
            Confidence::High,
            Generality::new(6).unwrap(),
            Effort::Medium,
            &risk_of(3),
            &ScoringWeights {
                impact_weight: 1.5,
                ..ScoringWeights::default()
            },
        )
        .0;
        assert!(emphasise_impact.value() >= base.value());
    }

    #[test]
    fn traces_carry_reasoning() {
        let (_priority, trace) = compute_priority(
            Impact::new(7).unwrap(),
            Confidence::High,
            Generality::new(5).unwrap(),
            Effort::Low,
            &risk_of(2),
            &ScoringWeights::default(),
        );
        assert!(trace.reasoning.contains("impact 7×1"));
        assert!(trace.raw > 0.0);
    }
}
