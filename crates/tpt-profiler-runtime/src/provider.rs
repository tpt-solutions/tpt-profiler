//! External tools as evidence providers (spec §68).
//!
//! A provider wraps an established tool (`perf`, `xctrace`, ETW tooling,
//! …) and is only used when it is actually installed and usable on the
//! current platform. Availability is probed at run time; a missing tool
//! surfaces as [`ToolUnavailableError`] rather than a crash, so the
//! remaining analyses still run.

use std::process::Command;
use std::time::Duration;

use tpt_profiler_model::error::ToolUnavailableError;

use crate::process;

/// Operating systems the profiler has backends for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Microsoft Windows.
    Windows,
    /// Linux.
    Linux,
    /// Apple macOS.
    MacOs,
}

impl Platform {
    /// The platform this binary is running on, if it is one of the
    /// supported targets.
    #[must_use]
    pub fn current() -> Option<Self> {
        match std::env::consts::OS {
            "windows" => Some(Self::Windows),
            "linux" => Some(Self::Linux),
            "macos" => Some(Self::MacOs),
            _ => None,
        }
    }
}

/// An external tool that can supply evidence (spec §68).
pub trait EvidenceProvider: std::fmt::Debug {
    /// Short human-readable name used in reports (e.g. `perf`).
    fn name(&self) -> &'static str;

    /// Executable to look up on `PATH`.
    fn program(&self) -> &'static str;

    /// Platforms this provider has a backend for.
    fn platforms(&self) -> &'static [Platform];

    /// Arguments that make the tool print its version and exit.
    fn probe_args(&self) -> &'static [&'static str] {
        &["--version"]
    }
}

/// A provider that passed its availability probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInfo {
    /// Executable that answered the probe.
    pub program: String,
    /// First non-empty line of the probe output.
    pub version: String,
}

/// The provider chosen by [`select_backend`], with the probe result.
#[derive(Debug)]
pub struct Backend<'a> {
    /// The selected provider.
    pub provider: &'a dyn EvidenceProvider,
    /// What the probe found.
    pub tool: ToolInfo,
}

/// Probes `program` by running it with `args` and reading its version.
///
/// * Not on `PATH` → [`ToolUnavailableError::Tool`] with an install hint.
/// * Non-zero exit or timeout → [`ToolUnavailableError::Tool`] naming the
///   failure.
pub fn detect(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<ToolInfo, ToolUnavailableError> {
    let mut command = Command::new(program);
    command.args(args);
    let outcome = process::run(&mut command, timeout, None).map_err(|error| {
        let hint = if error.kind() == std::io::ErrorKind::NotFound {
            format!("`{program}` was not found on PATH")
        } else {
            format!("could not start `{program}`: {error}")
        };
        unavailable(program, hint)
    })?;

    if outcome.timed_out {
        return Err(unavailable(
            program,
            String::from("version probe timed out"),
        ));
    }
    if !outcome.success {
        return Err(unavailable(
            program,
            format!("version probe exited with {:?}", outcome.exit_code),
        ));
    }

    let version = first_line(&outcome.stdout)
        .or_else(|| first_line(&outcome.stderr))
        .unwrap_or_default();
    Ok(ToolInfo {
        program: program.to_owned(),
        version,
    })
}

/// Picks the first provider that supports `platform` and passes its probe.
///
/// Providers are tried in the order given, so callers express preference
/// by ordering (e.g. the native profiler before a portable fallback).
///
/// On failure returns the reason each applicable provider was rejected.
/// The list is empty when no provider targets `platform` at all.
pub fn select_backend<'a>(
    providers: &[&'a dyn EvidenceProvider],
    platform: Platform,
    timeout: Duration,
) -> Result<Backend<'a>, Vec<ToolUnavailableError>> {
    let mut rejections = Vec::new();
    for &provider in providers {
        if !provider.platforms().contains(&platform) {
            continue;
        }
        match detect(provider.program(), provider.probe_args(), timeout) {
            Ok(tool) => return Ok(Backend { provider, tool }),
            Err(reason) => rejections.push(reason),
        }
    }
    Err(rejections)
}

fn unavailable(tool: &str, hint: String) -> ToolUnavailableError {
    ToolUnavailableError::Tool {
        tool: tool.to_owned(),
        hint,
    }
}

fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(30);

    #[derive(Debug)]
    struct Fake {
        program: &'static str,
        platforms: &'static [Platform],
    }

    impl EvidenceProvider for Fake {
        fn name(&self) -> &'static str {
            self.program
        }
        fn program(&self) -> &'static str {
            self.program
        }
        fn platforms(&self) -> &'static [Platform] {
            self.platforms
        }
    }

    const ALL: &[Platform] = &[Platform::Windows, Platform::Linux, Platform::MacOs];

    fn cargo() -> Fake {
        // `CARGO` is set by cargo itself, so the probe does not depend on PATH.
        Fake {
            program: env!("CARGO"),
            platforms: ALL,
        }
    }

    fn missing() -> Fake {
        Fake {
            program: "tpt-profiler-no-such-tool",
            platforms: ALL,
        }
    }

    #[test]
    fn detect_reports_version_of_present_tool() {
        let info = detect(env!("CARGO"), &["--version"], TIMEOUT).expect("cargo is present");
        assert!(info.version.starts_with("cargo "), "got {:?}", info.version);
    }

    #[test]
    fn detect_missing_tool_is_tool_unavailable() {
        let error = detect("tpt-profiler-no-such-tool", &["--version"], TIMEOUT)
            .expect_err("tool should be missing");
        let ToolUnavailableError::Tool { tool, hint } = error;
        assert_eq!(tool, "tpt-profiler-no-such-tool");
        assert!(hint.contains("not found"), "got {hint:?}");
    }

    #[test]
    fn select_falls_back_past_missing_tool() {
        let missing = missing();
        let cargo = cargo();
        let providers: [&dyn EvidenceProvider; 2] = [&missing, &cargo];
        let backend = select_backend(&providers, Platform::Linux, TIMEOUT)
            .expect("fallback should be selected");
        assert_eq!(backend.provider.name(), env!("CARGO"));
    }

    #[test]
    fn select_collects_rejections_when_nothing_is_available() {
        let missing = missing();
        let providers: [&dyn EvidenceProvider; 1] = [&missing];
        let rejections = select_backend(&providers, Platform::Windows, TIMEOUT)
            .expect_err("no provider is installed");
        assert_eq!(rejections.len(), 1);
    }

    #[test]
    fn select_skips_providers_for_other_platforms() {
        let elsewhere = Fake {
            program: "tpt-profiler-no-such-tool",
            platforms: &[Platform::MacOs],
        };
        let providers: [&dyn EvidenceProvider; 1] = [&elsewhere];
        let rejections = select_backend(&providers, Platform::Linux, TIMEOUT)
            .expect_err("no provider targets Linux");
        assert_eq!(rejections.len(), 0);
    }

    #[test]
    fn current_platform_is_supported_on_ci_hosts() {
        assert!(Platform::current().is_some());
    }
}
