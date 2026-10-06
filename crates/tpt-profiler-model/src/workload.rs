//! Explicit workload representation (spec §23).
//!
//! A profile is only meaningful relative to a workload; every measurement
//! records which workload it belongs to so runs are comparable.

use alloc::string::String;

/// A named unit of work the profiler measures against (spec §23).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Workload {
    /// Stable name, e.g. `decode_1080p`.
    pub name: String,
    /// Input identifier (path, fixture name, size), when applicable.
    pub input: Option<String>,
    /// Suggested per-repetition duration budget in milliseconds.
    pub duration_hint_ms: Option<u64>,
    /// Number of repetitions to run.
    pub repetitions: Option<u32>,
}

impl Workload {
    /// A named workload without optional parameters.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            input: None,
            duration_hint_ms: None,
            repetitions: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_workload_has_defaults() {
        let workload = Workload::named("decode_1080p");
        assert_eq!(workload.name, "decode_1080p");
        assert_eq!(workload.input, None);
        assert_eq!(workload.repetitions, None);
    }
}
