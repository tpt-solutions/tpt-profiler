//! Profile runs: one complete execution of the profiler pipeline
//! (spec §6, §49).

use alloc::string::String;
use alloc::vec::Vec;

use crate::environment::Environment;
use crate::measurement::{Measurement, UnixSeconds};
use crate::opportunity::Opportunity;

/// Everything one profiler invocation produced (spec §6): measurements
/// and opportunities against a captured environment. Serialised under a
/// [`crate::schema::SchemaEnvelope`].
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProfileRun {
    /// Run identifier (unique per invocation, e.g. timestamp-derived).
    pub id: String,
    /// When the run started (Unix seconds).
    pub started_at: UnixSeconds,
    /// `tpt-profiler` version that produced this run.
    pub tool_version: String,
    /// Environment the run captured (spec §48, §49).
    pub environment: Environment,
    /// Measurements collected during the run (spec §24).
    pub measurements: Vec<Measurement>,
    /// Opportunities detected during the run (spec §25).
    pub opportunities: Vec<Opportunity>,
}

impl ProfileRun {
    /// An empty run with the given identity and environment.
    #[must_use]
    pub fn new(id: impl Into<String>, started_at: UnixSeconds, environment: Environment) -> Self {
        Self {
            id: id.into(),
            started_at,
            tool_version: crate::version().into(),
            environment,
            measurements: Vec::new(),
            opportunities: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_run_stamps_tool_version() {
        let run = ProfileRun::new("run-1", 1_760_000_000, Environment::default());
        assert_eq!(run.tool_version, crate::version());
        assert_eq!(run.measurements.len(), 0);
        assert_eq!(run.opportunities.len(), 0);
    }
}
