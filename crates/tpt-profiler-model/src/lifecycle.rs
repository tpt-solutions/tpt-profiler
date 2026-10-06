//! Opportunity lifecycle states (spec §38).

use alloc::string::String;

use crate::id::OpportunityId;

/// Where an opportunity is in its lifecycle (spec §38):
/// detect → recommend → optimise → profile → compare → validate.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum LifecycleState {
    /// Detected, not yet triaged.
    Open,
    /// Triaged as worth doing.
    Accepted,
    /// Triaged as not worth doing (spec §52: a reason should be recorded).
    Rejected {
        /// Why it was rejected (e.g. "already optimised").
        reason: String,
    },
    /// Work has started.
    InProgress,
    /// The change landed; awaiting re-measurement.
    Optimised,
    /// Re-measured and the improvement was confirmed.
    Validated,
    /// The change landed but measurements got worse (spec §37).
    Regressed,
    /// Another opportunity subsumed this one.
    Superseded {
        /// The opportunity that replaced this one.
        by: OpportunityId,
    },
}

impl LifecycleState {
    /// The state newly detected opportunities start in.
    #[must_use]
    pub const fn initial() -> Self {
        Self::Open
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_open() {
        assert_eq!(LifecycleState::initial(), LifecycleState::Open);
    }

    #[test]
    fn rejected_carries_reason() {
        let state = LifecycleState::Rejected {
            reason: String::from("already optimised"),
        };
        assert_eq!(
            state,
            LifecycleState::Rejected {
                reason: String::from("already optimised")
            }
        );
    }
}
