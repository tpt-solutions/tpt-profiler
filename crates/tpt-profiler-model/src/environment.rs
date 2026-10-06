//! Execution-environment capture (spec §48, §49).
//!
//! Profiling results are environment-dependent, so every run record embeds
//! the environment it was captured in. All fields are plain data: capture
//! happens in the `std`-side crates and lands here as strings and numbers
//! so the model stays `no_std`.

use alloc::string::String;
use alloc::vec::Vec;

/// The environment a profiling run executed in (spec §48).
#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Environment {
    /// Operating system identifier (e.g. `windows`, `linux`, `macos`).
    pub os: Option<String>,
    /// CPU architecture / target triple (spec §49).
    pub arch: Option<String>,
    /// CPU model string.
    pub cpu_model: Option<String>,
    /// Logical core count.
    pub cpu_cores: Option<u32>,
    /// Total physical memory in bytes, when discoverable.
    pub memory_bytes: Option<u64>,
    /// `rustc --version` output.
    pub rustc_version: Option<String>,
    /// `cargo --version` output.
    pub cargo_version: Option<String>,
    /// Compilation target triple.
    pub target_triple: Option<String>,
    /// Optimisation/profile flags in effect.
    pub opt_flags: Vec<String>,
    /// Git commit hash of the analysed tree.
    pub git_commit: Option<String>,
    /// Whether the analysed tree had uncommitted changes.
    pub git_dirty: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_environment_is_empty() {
        let environment = Environment::default();
        assert_eq!(environment.os, None);
        assert!(environment.opt_flags.is_empty());
    }
}
