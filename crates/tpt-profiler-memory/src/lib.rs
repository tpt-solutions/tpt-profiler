//! Memory and allocation analysis for `tpt-profiler` (spec §8.4, §14).
//!
//! Allocation counts/sizes/frequencies, peak RSS, retained memory,
//! object lifetimes, and fragmentation indicators; integrates existing
//! tools (dhat, heaptrack, valgrind) as evidence providers rather than
//! reimplementing them (spec §68).
//!
//! **Status:** skeleton. Lands in Phase 2 (spec §63).
