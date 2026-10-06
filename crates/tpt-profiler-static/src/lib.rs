//! Static analysis engine for `tpt-profiler` (spec §9–13).
//!
//! `syn`-based, deterministic source analysis producing standardised
//! evidence (spec §67): allocation patterns, clone analysis, data-structure
//! mismatches, loop opportunities, SIMD candidates, and parallelism
//! candidates — each with evidence and confidence, never blind flagging
//! (spec §9).
//!
//! **Status:** skeleton. Detectors land with the MVP (spec §62).
