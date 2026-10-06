//! Report rendering for `tpt-profiler` (spec §42).
//!
//! Terminal (width-aware, `NO_COLOR`-respecting), JSON (versioned schema,
//! spec §69), and Markdown outputs that always distinguish measured facts
//! from predictions (spec §29).
//!
//! **Status:** the `scan` tree renderer is implemented (spec §7); the
//! remaining formats land with the MVP (spec §62).

pub mod scan;

pub use scan::render_scan;
