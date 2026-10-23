//! Outcomes that distinguish "nothing found" from "the run failed"
//! (spec §74).
//!
//! An analyzer that completes without finding anything has *succeeded*
//! with no evidence. Reserving [`crate::error::Error`] for real failures
//! keeps exit codes meaningful and prevents "no opportunities" from being
//! reported as a crash.

/// Result of an analysis step: either it found things or it explicitly
/// found nothing.
///
/// `T` is typically a collection of evidence or opportunities.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AnalysisOutcome<T> {
    /// The analysis ran to completion and detected no opportunities.
    /// This is a valid, successful result — not an error.
    NoOpportunity,
    /// The analysis ran to completion and produced findings.
    Findings(T),
}

impl<T> AnalysisOutcome<T> {
    /// Returns `true` if the analysis completed without findings.
    #[must_use]
    pub const fn is_no_opportunity(&self) -> bool {
        matches!(self, Self::NoOpportunity)
    }

    /// The findings, if any.
    #[must_use]
    pub const fn findings(&self) -> Option<&T> {
        match self {
            Self::NoOpportunity => None,
            Self::Findings(findings) => Some(findings),
        }
    }

    /// Consumes the outcome, returning the findings or a fallback value
    /// for the no-opportunity case.
    #[must_use]
    pub fn unwrap_or_else(self, fallback: impl FnOnce() -> T) -> T {
        match self {
            Self::NoOpportunity => fallback(),
            Self::Findings(findings) => findings,
        }
    }

    /// Maps the findings, leaving [`AnalysisOutcome::NoOpportunity`] as is.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> AnalysisOutcome<U> {
        match self {
            Self::NoOpportunity => AnalysisOutcome::NoOpportunity,
            Self::Findings(findings) => AnalysisOutcome::Findings(f(findings)),
        }
    }
}

impl<T> From<Option<T>> for AnalysisOutcome<T> {
    fn from(value: Option<T>) -> Self {
        match value {
            None => Self::NoOpportunity,
            Some(findings) => Self::Findings(findings),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_opportunity_is_not_an_error() {
        let outcome: AnalysisOutcome<Vec<u32>> = AnalysisOutcome::NoOpportunity;
        assert!(outcome.is_no_opportunity());
        assert!(outcome.findings().is_none());
    }

    #[test]
    fn findings_are_preserved_through_map() {
        let outcome: AnalysisOutcome<Vec<u32>> = AnalysisOutcome::Findings(alloc::vec![1, 2, 3]);
        let doubled = outcome.map(|v| v.into_iter().map(|x| x * 2).collect::<Vec<_>>());
        assert_eq!(doubled.findings(), Some(&alloc::vec![2, 4, 6]));
    }

    #[test]
    fn from_option_distinguishes_empty_from_failure() {
        let none: AnalysisOutcome<u8> = None.into();
        assert!(none.is_no_opportunity());
        let some: AnalysisOutcome<u8> = Some(7).into();
        assert_eq!(some.findings(), Some(&7));
    }
}
