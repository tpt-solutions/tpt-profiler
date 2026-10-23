//! Report rendering for `tpt-profiler` (spec §42).
//!
//! Terminal (width-aware, `NO_COLOR`-respecting), JSON (versioned schema,
//! spec §69), and Markdown outputs that always distinguish measured facts
//! from predictions (spec §29).
//!
//! **Status:** the `scan` tree renderer (spec §7) and the opportunity
//! report formats — terminal, JSON, Markdown (spec §42, §69, §76) — are
//! implemented.

pub mod json;
pub mod markdown;
pub mod scan;
pub mod terminal;

pub use json::{
    opportunity_summary, render_json, render_opportunities_json, render_summary_json,
    schema_version, summarise,
};
pub use markdown::render_markdown;
pub use scan::render_scan;
pub use terminal::{TerminalOptions, render_opportunities, wrap};
