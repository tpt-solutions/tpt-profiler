//! Rating scales: impact, confidence, effort, risk, generality, priority
//! (spec §27–§32).

use core::fmt;

/// Impact on a 1–10 scale (spec §28): 1 negligible … 10 transformative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Impact(u8);

/// Confidence on a 1–10 scale with named levels (spec §29): the levels are
/// the API; the numeric scale exists for scoring (spec §27) and reports
/// like `Confidence: 9/10` (spec §25).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Confidence {
    /// Weakly supported; heuristic-only evidence.
    VeryLow,
    /// Some support, significant doubt.
    Low,
    /// Plausible; static pattern plus partial corroboration.
    Medium,
    /// Strongly supported by profiling or counters.
    High,
    /// Observed regression or overwhelming evidence.
    VeryHigh,
}

/// Engineering effort on a 1–10 scale with named levels (spec §30).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Effort {
    /// Minutes; mechanical change.
    Trivial,
    /// Hours; localised change.
    Low,
    /// A day or two; touches several functions.
    Medium,
    /// Days; API or architecture involvement.
    High,
    /// Weeks; structural change with wide verification.
    VeryHigh,
}

/// Overall risk on a 1–10 scale (spec §31), with optional per-dimension
/// detail. For numerical software, [`RiskDimensions::numerical`] must
/// reflect numerical-equivalence risk — faster never implies equivalent
/// (spec §55).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Risk {
    /// Overall risk, 1–10.
    pub overall: RiskScore,
    /// Per-dimension risks, where assessed.
    pub dimensions: RiskDimensions,
}

/// A risk magnitude, 1–10 (spec §31).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RiskScore(u8);

/// Per-dimension risks (spec §31); `None` where not assessed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RiskDimensions {
    /// Could the change introduce bugs?
    pub correctness: Option<RiskScore>,
    /// Could it change numerical results? (spec §55)
    pub numerical: Option<RiskScore>,
    /// Does it touch concurrency (races, ordering)?
    pub concurrency: Option<RiskScore>,
    /// Does it require or modify `unsafe`? (spec §54)
    pub unsafe_region: Option<RiskScore>,
    /// Does it reduce platform portability?
    pub portability: Option<RiskScore>,
    /// Does it break public API compatibility?
    pub api: Option<RiskScore>,
}

/// How broadly an optimisation applies, 1–10 (spec §32).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Generality(u8);

/// Computed priority, 0–100 (spec §27). Produced by the opportunity
/// engine; the formula is deliberately not part of the API contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Priority(u8);

/// Error returned when a rating is constructed outside its valid range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ScaleError {
    /// Value outside the stated 1–10 range.
    #[error("value {value} out of range {minimum}..={maximum} for {scale}")]
    OutOfRange {
        /// The rejected value.
        value: u8,
        /// Inclusive lower bound.
        minimum: u8,
        /// Inclusive upper bound.
        maximum: u8,
        /// Scale name.
        scale: &'static str,
    },
}

impl Impact {
    /// Lowest possible impact.
    pub const MIN: u8 = 1;
    /// Highest possible impact.
    pub const MAX: u8 = 10;

    /// Constructs an `Impact`, rejecting values outside `1..=10`.
    pub const fn new(value: u8) -> Result<Self, ScaleError> {
        match check(value, Self::MIN, Self::MAX, "impact") {
            Ok(()) => Ok(Self(value)),
            Err(error) => Err(error),
        }
    }

    /// The underlying value, `1..=10`.
    #[must_use]
    pub const fn value(self) -> u8 {
        self.0
    }

    /// Spec §28 level name (`negligible` … `transformative`).
    #[must_use]
    pub const fn level_name(self) -> &'static str {
        const NAMES: [&str; 10] = [
            "negligible",
            "very low",
            "low",
            "modest",
            "moderate",
            "meaningful",
            "high",
            "very high",
            "major",
            "transformative",
        ];
        NAMES[(self.0 - 1) as usize]
    }
}

impl Generality {
    /// Lowest possible generality.
    pub const MIN: u8 = 1;
    /// Highest possible generality.
    pub const MAX: u8 = 10;

    /// Constructs a `Generality`, rejecting values outside `1..=10`.
    pub const fn new(value: u8) -> Result<Self, ScaleError> {
        match check(value, Self::MIN, Self::MAX, "generality") {
            Ok(()) => Ok(Self(value)),
            Err(error) => Err(error),
        }
    }

    /// The underlying value, `1..=10`.
    #[must_use]
    pub const fn value(self) -> u8 {
        self.0
    }
}

impl RiskScore {
    /// Lowest possible risk.
    pub const MIN: u8 = 1;
    /// Highest possible risk.
    pub const MAX: u8 = 10;

    /// Constructs a `RiskScore`, rejecting values outside `1..=10`.
    pub const fn new(value: u8) -> Result<Self, ScaleError> {
        match check(value, Self::MIN, Self::MAX, "risk") {
            Ok(()) => Ok(Self(value)),
            Err(error) => Err(error),
        }
    }

    /// The underlying value, `1..=10`.
    #[must_use]
    pub const fn value(self) -> u8 {
        self.0
    }
}

impl Priority {
    /// Lowest possible priority.
    pub const MIN: u8 = 0;
    /// Highest possible priority.
    pub const MAX: u8 = 100;

    /// Constructs a `Priority`, rejecting values outside `0..=100`.
    pub const fn new(value: u8) -> Result<Self, ScaleError> {
        match check(value, Self::MIN, Self::MAX, "priority") {
            Ok(()) => Ok(Self(value)),
            Err(error) => Err(error),
        }
    }

    /// The underlying value, `0..=100`.
    #[must_use]
    pub const fn value(self) -> u8 {
        self.0
    }
}

impl Risk {
    /// Constructs a risk with the given overall score and default
    /// (unassessed) dimensions.
    #[must_use]
    pub const fn overall(score: RiskScore) -> Self {
        Self {
            overall: score,
            dimensions: RiskDimensions {
                correctness: None,
                numerical: None,
                concurrency: None,
                unsafe_region: None,
                portability: None,
                api: None,
            },
        }
    }
}

impl Confidence {
    /// All levels, weakest to strongest.
    pub const LEVELS: [Confidence; 5] = [
        Self::VeryLow,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::VeryHigh,
    ];

    /// Numeric scale value (spec §25 shows `Confidence: 9/10`): levels map
    /// to 2/4/6/8/10.
    #[must_use]
    pub const fn scale(self) -> u8 {
        match self {
            Self::VeryLow => 2,
            Self::Low => 4,
            Self::Medium => 6,
            Self::High => 8,
            Self::VeryHigh => 10,
        }
    }

    /// Nearest level for a numeric value (rounding down to even).
    #[must_use]
    pub const fn from_scale(value: u8) -> Self {
        if value >= 9 {
            Self::VeryHigh
        } else if value >= 7 {
            Self::High
        } else if value >= 5 {
            Self::Medium
        } else if value >= 3 {
            Self::Low
        } else {
            Self::VeryLow
        }
    }

    /// Spec §29 level name.
    #[must_use]
    pub const fn level_name(self) -> &'static str {
        match self {
            Self::VeryLow => "very low",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::VeryHigh => "very high",
        }
    }
}

impl Effort {
    /// All levels, cheapest to most expensive.
    pub const LEVELS: [Effort; 5] = [
        Self::Trivial,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::VeryHigh,
    ];

    /// Numeric scale value: levels map to 1/3/5/7/9.
    #[must_use]
    pub const fn scale(self) -> u8 {
        match self {
            Self::Trivial => 1,
            Self::Low => 3,
            Self::Medium => 5,
            Self::High => 7,
            Self::VeryHigh => 9,
        }
    }

    /// Nearest level for a numeric value.
    #[must_use]
    pub const fn from_scale(value: u8) -> Self {
        if value >= 8 {
            Self::VeryHigh
        } else if value >= 6 {
            Self::High
        } else if value >= 4 {
            Self::Medium
        } else if value >= 2 {
            Self::Low
        } else {
            Self::Trivial
        }
    }

    /// Spec §30 level name.
    #[must_use]
    pub const fn level_name(self) -> &'static str {
        match self {
            Self::Trivial => "trivial",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::VeryHigh => "very high",
        }
    }
}

const fn check(value: u8, minimum: u8, maximum: u8, scale: &'static str) -> Result<(), ScaleError> {
    if value < minimum || value > maximum {
        Err(ScaleError::OutOfRange {
            value,
            minimum,
            maximum,
            scale,
        })
    } else {
        Ok(())
    }
}

impl fmt::Display for Impact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/10 ({})", self.0, self.level_name())
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/10 ({})", self.scale(), self.level_name())
    }
}

impl fmt::Display for Effort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/10 ({})", self.scale(), self.level_name())
    }
}

impl fmt::Display for Generality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/10", self.0)
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/100", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString as _;

    #[test]
    fn impact_range_is_enforced() {
        assert!(matches!(Impact::new(0), Err(ScaleError::OutOfRange { .. })));
        assert!(matches!(
            Impact::new(11),
            Err(ScaleError::OutOfRange { .. })
        ));
        assert_eq!(Impact::new(7).map(Impact::value), Ok(7));
    }

    #[test]
    fn impact_level_names_follow_spec_28() {
        assert_eq!(Impact::new(1).unwrap().level_name(), "negligible");
        assert_eq!(Impact::new(5).unwrap().level_name(), "moderate");
        assert_eq!(Impact::new(10).unwrap().level_name(), "transformative");
    }

    #[test]
    fn confidence_levels_round_trip_through_scale() {
        for level in Confidence::LEVELS {
            assert_eq!(Confidence::from_scale(level.scale()), level);
        }
        assert_eq!(Confidence::from_scale(9), Confidence::VeryHigh);
        assert_eq!(Confidence::from_scale(1), Confidence::VeryLow);
    }

    #[test]
    fn effort_levels_round_trip_through_scale() {
        for level in Effort::LEVELS {
            assert_eq!(Effort::from_scale(level.scale()), level);
        }
    }

    #[test]
    fn priority_allows_full_hundred_range() {
        assert_eq!(Priority::new(0).unwrap().value(), 0);
        assert_eq!(Priority::new(100).unwrap().value(), 100);
        assert!(Priority::new(101).is_err());
    }

    #[test]
    fn risk_has_overall_and_dimensions() {
        let risk = Risk {
            overall: RiskScore::new(3).unwrap(),
            dimensions: RiskDimensions {
                numerical: Some(RiskScore::new(7).unwrap()),
                ..RiskDimensions::default()
            },
        };
        assert_eq!(risk.overall.value(), 3);
        assert_eq!(risk.dimensions.numerical.map(RiskScore::value), Some(7));
        assert_eq!(risk.dimensions.correctness, None);
    }

    #[test]
    fn displays_match_spec_examples() {
        assert_eq!(Confidence::VeryHigh.to_string(), "10/10 (very high)");
        assert_eq!(Effort::Medium.to_string(), "5/10 (medium)");
        assert_eq!(Priority::new(86).unwrap().to_string(), "86/100");
    }
}
