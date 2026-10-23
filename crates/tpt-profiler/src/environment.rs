//! Execution-environment capture (spec §48, §49).
//!
//! Fills the `no_std` [`Environment`] model type with real host data:
//! OS/arch come from compile-time constants, CPU/core data from the
//! standard library, tool versions and git state from child processes.
//! Anything not discoverable without heavy platform machinery stays
//! `None` rather than pulling a big dependency (spec §72).

use std::path::Path;
use std::process::Command;

use tpt_profiler_model::Environment;

/// Captures the current execution environment.
///
/// Tool/git queries are best-effort: a missing `git` or `rustc` leaves
/// the corresponding field `None` instead of failing the run (spec §74:
/// capture is evidence gathering, not a hard requirement).
#[must_use]
pub fn capture_environment(repo_root: Option<&Path>) -> Environment {
    let mut environment = Environment {
        os: Some(std::env::consts::OS.to_string()),
        arch: Some(target_triple()),
        cpu_cores: Some(
            #[allow(
                clippy::cast_possible_truncation,
                reason = "core counts beyond u32::MAX are not physical hardware"
            )]
            std::thread::available_parallelism().map_or(0, |parallelism| parallelism.get() as u32),
        ),
        ..Environment::default()
    };
    environment.target_triple = environment.arch.clone();
    environment.cpu_model = cpu_model();
    environment.memory_bytes = total_memory_bytes();
    if let Some((rustc, cargo)) = tool_versions() {
        environment.rustc_version = Some(rustc);
        environment.cargo_version = Some(cargo);
    }
    if let Some(root) = repo_root {
        if let Some((commit, dirty)) = git_state(root) {
            environment.git_commit = Some(commit);
            environment.git_dirty = Some(dirty);
        }
    }
    environment
}

/// Host target triple (`rustc -vV`, host line), falling back to
/// compile-time arch when rustc is unavailable.
fn target_triple() -> String {
    rustc_verbose()
        .and_then(|output| {
            output
                .lines()
                .find_map(|line| line.strip_prefix("host: "))
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| format!("{}-unknown", std::env::consts::ARCH))
}

fn rustc_verbose() -> Option<String> {
    run(Command::new("rustc").arg("-vV"))
}

/// `(rustc --version, cargo --version)`.
fn tool_versions() -> Option<(String, String)> {
    let rustc = run(Command::new("rustc").arg("--version"))?;
    let cargo = run(Command::new("cargo").arg("--version"))?;
    Some((rustc, cargo))
}

/// `(HEAD commit, is-dirty)` for the repository at `root`.
fn git_state(root: &Path) -> Option<(String, bool)> {
    let commit = run(Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "HEAD"]))?;
    let status = run(Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["status", "--porcelain"]))?;
    let dirty = !status.trim().is_empty();
    Some((commit, dirty))
}

fn cpu_model() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let info = std::fs::read_to_string("/proc/cpuinfo").ok()?;
        info.lines()
            .find_map(|line| {
                line.strip_prefix("model name")
                    .and_then(|rest| rest.split_once(':'))
            })
            .map(|(_, model)| model.trim().to_string())
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Windows/macOS need platform APIs (wmic/sysctl); left `None`
        // rather than shelling out brittle commands (spec §72).
        None
    }
}

fn total_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let info = std::fs::read_to_string("/proc/meminfo").ok()?;
        let kilobytes: u64 = info
            .lines()
            .find_map(|line| line.strip_prefix("MemTotal:"))?
            .split_whitespace()
            .next()?
            .parse()
            .ok()?;
        Some(kilobytes * 1024)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Runs a command, returning trimmed stdout; `None` when the binary is
/// missing or the command failed.
fn run(command: &mut Command) -> Option<String> {
    let output = command.output().ok()?;
    if output.status.success() {
        String::from_utf8(output.stdout)
            .ok()
            .map(|text| text.trim().to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_and_arch_are_always_captured() {
        let environment = capture_environment(None);
        assert_eq!(environment.os.as_deref(), Some(std::env::consts::OS));
        assert!(environment.arch.is_some_and(|arch| !arch.is_empty()));
        assert!(environment.cpu_cores.unwrap_or(0) > 0);
    }

    #[test]
    fn tool_versions_are_captured_on_dev_machines() {
        let environment = capture_environment(None);
        let rustc = environment
            .rustc_version
            .expect("rustc in PATH during tests");
        assert!(rustc.starts_with("rustc"), "unexpected: {rustc}");
        let cargo = environment
            .cargo_version
            .expect("cargo in PATH during tests");
        assert!(cargo.starts_with("cargo"), "unexpected: {cargo}");
    }

    #[test]
    fn git_state_captures_this_repository() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let environment = capture_environment(Some(&repo));
        let commit = environment.git_commit.expect("running inside a git repo");
        assert_eq!(commit.len(), 40, "full SHA expected: {commit}");
        assert!(environment.git_dirty.is_some(), "dirty flag must be a bool");
    }

    #[test]
    fn missing_repo_leaves_git_fields_empty() {
        let environment = capture_environment(None);
        assert_eq!(environment.git_commit, None);
        assert_eq!(environment.git_dirty, None);
    }
}
