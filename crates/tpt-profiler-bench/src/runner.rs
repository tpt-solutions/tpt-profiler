//! Benchmark discovery and execution (spec §8.3, §62).
//!
//! `cargo bench` is the evidence provider (spec §68): this crate runs it
//! where benches exist and parses Criterion output when it is produced.
//! Absent benches or absent Criterion data are *results*, not errors
//! (spec §74: `ToolUnavailable` / "no benchmarks found" are distinct
//! from profiling failures).

use std::path::Path;
use std::time::Duration;

use tpt_profiler_model::error::{BenchmarkError, Error, ToolUnavailableError};
use tpt_profiler_model::hierarchy::CrateInfo;

use crate::criterion::{CriterionEstimate, collect_estimates};

/// Result of a bench run.
#[derive(Debug, Clone, Default)]
pub struct BenchReport {
    /// Parsed Criterion estimates (`name → estimate`); empty when no
    /// Criterion data was produced.
    pub estimates: std::collections::BTreeMap<String, CriterionEstimate>,
    /// Whether `cargo bench` itself reported no bench targets.
    pub no_benchmarks: bool,
    /// Captured `cargo bench` diagnostics for reporting.
    pub cargo_output: String,
}

/// Runs `cargo bench` in `workspace_root` and ingests Criterion results.
///
/// * No bench targets discovered statically → [`BenchReport::no_benchmarks`]
///   without running anything (spec §8.3, §74).
/// * `cargo` missing → [`Error::ToolUnavailable`].
/// * Successful run with no Criterion tree → empty estimates (Criterion
///   may be absent; that is fine).
pub fn run_benches(
    workspace_root: &Path,
    members: &[CrateInfo],
    timeout: Duration,
) -> Result<BenchReport, Error> {
    let any_benches = members.iter().any(|info| info.has_benches);
    if !any_benches {
        return Ok(BenchReport {
            no_benchmarks: true,
            ..BenchReport::default()
        });
    }

    let mut command = std::process::Command::new("cargo");
    command.current_dir(workspace_root).arg("bench");
    let outcome =
        tpt_profiler_runtime::process::run(&mut command, timeout, None).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::ToolUnavailable(ToolUnavailableError::Tool {
                    tool: String::from("cargo"),
                    hint: String::from("cargo must be on PATH to run benchmarks"),
                })
            } else {
                Error::Benchmark(BenchmarkError::ExecutionFailed {
                    name: String::from("cargo bench"),
                    message: error.to_string(),
                })
            }
        })?;
    if !outcome.success {
        return Err(Error::Benchmark(BenchmarkError::ExecutionFailed {
            name: String::from("cargo bench"),
            message: if outcome.timed_out {
                String::from("timed out")
            } else {
                format!("exit code {:?}\n{}", outcome.exit_code, outcome.stderr)
            },
        }));
    }

    let criterion_dir = workspace_root.join("target").join("criterion");
    let estimates = collect_estimates(&criterion_dir);
    Ok(BenchReport {
        estimates,
        no_benchmarks: false,
        cargo_output: outcome.stdout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_profiler_model::hierarchy::CrateInfo;

    fn member(name: &str, has_benches: bool) -> CrateInfo {
        CrateInfo {
            name: String::from(name),
            has_benches,
            ..CrateInfo::default()
        }
    }

    #[test]
    fn no_bench_targets_is_a_result_not_an_error() {
        let temp = std::env::temp_dir().join("tpt-bench-none");
        std::fs::create_dir_all(&temp).expect("mkdir");
        let report = run_benches(&temp, &[member("plain", false)], Duration::from_secs(60))
            .expect("no benches is fine");
        assert!(report.no_benchmarks);
        assert_eq!(report.estimates.len(), 0);
        std::fs::remove_dir_all(&temp).ok();
    }

    #[test]
    fn criterion_data_missing_after_successful_run_is_empty_estimates() {
        // A workspace with benches whose `cargo bench` cannot run here —
        // but the *Criterion ingestion* half is what matters: an absent
        // criterion dir yields an empty map rather than an error.
        let temp = std::env::temp_dir().join("tpt-bench-missing-criterion");
        std::fs::create_dir_all(&temp).expect("mkdir");
        assert_eq!(
            collect_estimates(&temp.join("target").join("criterion")).len(),
            0
        );
        std::fs::remove_dir_all(&temp).ok();
    }
}
