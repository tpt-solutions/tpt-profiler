//! Opportunity engine for `tpt-profiler` (spec §25–35, §51).
//!
//! Converts evidence into opportunities: estimates impact, confidence,
//! effort, risk, and generality; computes priority; groups related
//! findings (spec §58); and never recommends where evidence is
//! insufficient (spec §3.1.12). The scoring formula is explicitly *not*
//! an API contract (spec §27).
//!
//! **Status:** skeleton. Lands with the MVP (spec §62).
