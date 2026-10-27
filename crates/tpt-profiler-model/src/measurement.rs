//! Measurements: observed facts, kept strictly separate from
//! interpretation (spec §24, §51).

use core::fmt;

use alloc::string::String;

use crate::environment::Environment;
use crate::scales::Confidence;

/// Unix timestamp in whole seconds since the epoch.
pub type UnixSeconds = i64;

/// What produced a measurement.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum MeasurementSource {
    /// Criterion benchmark output (spec §8.3).
    Criterion,
    /// Linux `perf`.
    Perf,
    /// Windows ETW/WPR.
    Etw,
    /// macOS Instruments/`sample`.
    Instruments,
    /// A counting global allocator (spec §14).
    CountingAllocator,
    /// Process wall-clock time.
    WallClock,
    /// Process CPU time.
    ProcessTime,
    /// Binary/section size on disk (spec §19).
    BinarySize,
    /// Build timings (spec §20).
    BuildTiming,
    /// Resident-memory high-water mark sampled from `/proc` while a child
    /// ran (spec §8.4, §14). A lower bound; see the runtime memory module.
    ProcStatus,
    /// Package energy counter (Linux RAPL via `/sys/class/powercap`, spec §56).
    /// System-wide, so it includes everything else running at the time.
    Rapl,
    /// Derived by static analysis (never a runtime fact; spec §75).
    Static,
    /// Any other provider (spec §68: external tools are evidence
    /// providers).
    Other(String),
}

/// Units for measured values.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Unit {
    /// Seconds.
    Seconds,
    /// Milliseconds.
    Milliseconds,
    /// Microseconds.
    Microseconds,
    /// Nanoseconds.
    Nanoseconds,
    /// Bytes.
    Bytes,
    /// A plain count (allocations, symbols, dependencies…).
    Count,
    /// Percentage.
    Percent,
    /// Raw counter values (instructions, cycles, cache misses).
    Events,
    /// Joules (spec §56, future).
    Joules,
    /// Watts (spec §56, future).
    Watts,
    /// Unitless ratio.
    Ratio,
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Seconds => "s",
            Self::Milliseconds => "ms",
            Self::Microseconds => "us",
            Self::Nanoseconds => "ns",
            Self::Bytes => "B",
            Self::Count => "count",
            Self::Percent => "%",
            Self::Events => "events",
            Self::Joules => "J",
            Self::Watts => "W",
            Self::Ratio => "ratio",
        })
    }
}

/// Whether a finding is exact or noisy (spec §75): static analysis is
/// deterministic; runtime measurements are statistical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum FindingKind {
    /// Reproducible exactly across runs (static analysis).
    Deterministic,
    /// Sampled/noisy; needs repeats and confidence (spec §50).
    Statistical,
}

/// One observed fact (spec §24). Carries *what was measured*, never what
/// to do about it — interpretation belongs to
/// [`crate::opportunity::Opportunity`] (spec §51).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Measurement {
    /// Metric name, snake_case (e.g. `cpu_time`, `peak_rss`, `alloc_count`).
    pub metric: String,
    /// Measured value.
    pub value: f64,
    /// Unit of [`Self::value`].
    pub unit: Unit,
    /// What produced the measurement.
    pub source: MeasurementSource,
    /// When the measurement was taken (Unix seconds).
    pub timestamp: UnixSeconds,
    /// Workload name this measurement belongs to (spec §23), when any.
    pub workload: Option<String>,
    /// Environment snapshot; run-level environment is inherited when
    /// `None` (spec §48).
    pub environment: Option<Environment>,
    /// How strongly this single measurement can be trusted (spec §29):
    /// single executions are at most [`Confidence::Low`].
    pub confidence: Confidence,
    /// Deterministic vs statistical (spec §75).
    pub kind: FindingKind,
}

impl Measurement {
    /// Creates a wall-clock-style runtime measurement with statistical
    /// semantics (spec §50): kind is always
    /// [`FindingKind::Statistical`].
    #[must_use]
    pub fn runtime(
        metric: impl Into<String>,
        value: f64,
        unit: Unit,
        source: MeasurementSource,
        timestamp: UnixSeconds,
    ) -> Self {
        Self {
            metric: metric.into(),
            value,
            unit,
            source,
            timestamp,
            workload: None,
            environment: None,
            confidence: Confidence::Low,
            kind: FindingKind::Statistical,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString as _;

    #[test]
    fn runtime_measurements_are_statistical_and_low_confidence() {
        let measurement = Measurement::runtime(
            "cpu_time",
            1.482,
            Unit::Seconds,
            MeasurementSource::Perf,
            1_760_000_000,
        );
        assert_eq!(measurement.kind, FindingKind::Statistical);
        assert_eq!(measurement.confidence, Confidence::Low);
        assert_eq!(measurement.unit.to_string(), "s");
    }

    #[test]
    fn unit_display() {
        assert_eq!(Unit::Bytes.to_string(), "B");
        assert_eq!(Unit::Percent.to_string(), "%");
        assert_eq!(Unit::Events.to_string(), "events");
    }
}
