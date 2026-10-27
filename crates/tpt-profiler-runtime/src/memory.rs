//! Peak resident memory of a child process (spec §8.4, §14), sampled from
//! the operating system while it runs.
//!
//! On Linux the kernel keeps a per-process high-water mark of resident
//! memory (`VmHWM` in `/proc/<pid>/status`). This module reads that mark
//! on each poll of a running child and keeps the largest value seen.
//!
//! The sample is a lower bound. A child that allocates heavily in its last
//! few milliseconds and exits before the next poll can show less than its
//! true peak; the poll interval is the limit of the resolution. Elsewhere
//! there is no `/proc`, and every reading is `None`. The workspace forbids
//! `unsafe`, so there is no `wait4`/`getrusage` path, which would be exact.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::process::{CancelToken, RunOutcome, run_observed};

/// Extracts the `VmHWM` value, in kilobytes, from the text of
/// `/proc/<pid>/status`. Returns `None` when the line is missing or malformed.
#[must_use]
pub fn parse_vm_hwm_kb(status: &str) -> Option<u64> {
    status.lines().find_map(|line| {
        let value = line.strip_prefix("VmHWM:")?;
        let mut parts = value.split_whitespace();
        let number: u64 = parts.next()?.parse().ok()?;
        (parts.next() == Some("kB")).then_some(number)
    })
}

/// Reads the high-water mark of a live process, in kilobytes. `None` when
/// the process has gone or the platform has no `/proc`.
#[must_use]
pub fn read_peak_rss_kb(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    parse_vm_hwm_kb(&status)
}

/// Whether this platform exposes the sampling source at all.
#[must_use]
pub fn sampling_supported() -> bool {
    Path::new("/proc/self/status").is_file()
}

/// Outcome of one sampled run.
#[derive(Debug)]
pub struct SampledRun {
    /// The process outcome, as [`crate::process::run`] reports it.
    pub outcome: RunOutcome,
    /// Largest high-water mark seen while the child ran, in kilobytes.
    /// `None` when no poll could read one.
    pub peak_rss_kb: Option<u64>,
}

/// Runs a command once and samples its peak resident memory (spec §14).
///
/// # Errors
/// Fails when the command cannot be spawned.
pub fn run_sampled(
    command: &mut Command,
    timeout: Duration,
    cancel: Option<&CancelToken>,
) -> std::io::Result<SampledRun> {
    let mut peak: Option<u64> = None;
    let outcome = run_observed(command, timeout, cancel, &mut |pid| {
        if let Some(kb) = read_peak_rss_kb(pid) {
            peak = Some(peak.map_or(kb, |best| best.max(kb)));
        }
    })?;
    Ok(SampledRun {
        outcome,
        peak_rss_kb: peak,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_high_water_mark_line() {
        let status = "Name:\tprobe\nVmPeak:\t  2048 kB\nVmHWM:\t    1234 kB\nThreads:\t1\n";
        assert_eq!(parse_vm_hwm_kb(status), Some(1234));
    }

    #[test]
    fn missing_or_malformed_values_are_none() {
        assert_eq!(parse_vm_hwm_kb("Name:\tprobe\n"), None);
        assert_eq!(parse_vm_hwm_kb("VmHWM:\tmany kB\n"), None);
        assert_eq!(parse_vm_hwm_kb("VmHWM:\t99 MB\n"), None, "units must be kB");
        assert_eq!(parse_vm_hwm_kb(""), None);
    }

    #[test]
    fn a_vanished_process_has_no_reading() {
        // A pid this large is never assigned in practice.
        assert_eq!(read_peak_rss_kb(u32::MAX), None);
    }

    #[test]
    fn sampled_runs_report_the_outcome_even_without_a_reading() {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/C", "echo", "sampled"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "echo sampled"]);
            command
        };
        let sampled = run_sampled(&mut command, Duration::from_secs(30), None).expect("spawns");
        assert!(sampled.outcome.success);
        assert!(sampled.outcome.stdout.contains("sampled"));
    }
}
