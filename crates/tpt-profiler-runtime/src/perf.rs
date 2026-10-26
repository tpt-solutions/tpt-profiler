//! Linux `perf` backend (spec §8.5, §63 Phase 2.2): hardware counters from
//! `perf stat` and hotspots from sampled stacks via `perf record` and
//! `perf script`.
//!
//! This module only builds arguments and parses output; running the tool
//! goes through [`crate::process::run`], and availability is probed through
//! [`crate::provider`], so a missing `perf` is reported rather than fatal.
//!
//! Counter values are read in `perf stat -x,` (CSV) form, which perf writes
//! to stderr. Repeated runs (`-r`) give a mean, so counters are judged as
//! statistical evidence, not as a single definitive run (spec §50).

use crate::provider::{EvidenceProvider, Platform};

/// Hardware events requested from `perf stat` (spec §8.5).
pub const HARDWARE_EVENTS: &[&str] = &[
    "instructions",
    "cycles",
    "branches",
    "branch-misses",
    "cache-references",
    "cache-misses",
];

/// The `perf` executable as an evidence provider.
#[derive(Debug, Clone, Copy, Default)]
pub struct Perf;

impl EvidenceProvider for Perf {
    fn name(&self) -> &'static str {
        "perf"
    }

    fn program(&self) -> &'static str {
        "perf"
    }

    fn platforms(&self) -> &'static [Platform] {
        &[Platform::Linux]
    }
}

/// One counter value read from `perf stat`.
#[derive(Debug, Clone, PartialEq)]
pub struct Counter {
    /// Event name as perf reports it (e.g. `cycles`).
    pub event: String,
    /// The counted value (the mean when `-r` was used).
    pub value: f64,
}

/// Arguments for `perf stat`, placed before the workload command.
///
/// `repeats` is passed as `-r`, so perf averages that many runs.
#[must_use]
pub fn stat_args(events: &[&str], repeats: u32) -> Vec<String> {
    vec![
        String::from("stat"),
        String::from("-x,"),
        String::from("-r"),
        repeats.max(1).to_string(),
        String::from("-e"),
        events.join(","),
        String::from("--"),
    ]
}

/// Parses `perf stat -x,` output (on stderr).
///
/// Each counter line is `value,unit,event,...`. Lines perf writes for
/// events it could not count (`<not counted>`, `<not supported>`) and any
/// other non-numeric value are skipped rather than read as zero.
#[must_use]
pub fn parse_stat(output: &str) -> Vec<Counter> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(',');
            let value = fields.next()?.trim();
            let _unit = fields.next()?;
            let event = fields.next()?.trim();
            let value: f64 = value.parse().ok()?;
            (!event.is_empty()).then(|| Counter {
                event: event.to_owned(),
                value,
            })
        })
        .collect()
}

/// Arguments for `perf record`, placed before the workload command.
///
/// Call-graph sampling (`-g`) is on so `perf script` can report the leaf
/// frame of each sample; `frequency_hz` is passed as `-F`.
#[must_use]
pub fn record_args(output: &str, frequency_hz: u32) -> Vec<String> {
    vec![
        String::from("record"),
        String::from("-F"),
        frequency_hz.max(1).to_string(),
        String::from("-g"),
        String::from("-o"),
        output.to_owned(),
        String::from("--"),
    ]
}

/// Arguments for `perf script` over a recording made by [`record_args`].
#[must_use]
pub fn script_args(recording: &str) -> Vec<String> {
    vec![
        String::from("script"),
        String::from("-i"),
        recording.to_owned(),
    ]
}

/// One function's share of the sampled time.
#[derive(Debug, Clone, PartialEq)]
pub struct Hotspot {
    /// Symbol of the leaf frame, without its offset or object file.
    pub symbol: String,
    /// Samples whose leaf frame is this symbol.
    pub samples: u64,
    /// Share of all samples, 0–100.
    pub percent: f64,
}

/// Turns `perf script` output into hotspots, ranked by sample count.
///
/// A sample starts at an unindented header line; the first indented line
/// after it is the leaf frame, shaped like `  7f3a…  symbol+0x1f (/bin/x)`.
/// Frames perf could not resolve are kept under `[unknown]`.
#[must_use]
pub fn parse_script(output: &str) -> Vec<Hotspot> {
    let mut leaves: Vec<String> = Vec::new();
    let mut awaiting_leaf = false;
    for line in output.lines() {
        if line.trim().is_empty() {
            awaiting_leaf = false;
        } else if !line.starts_with([' ', '\t']) {
            awaiting_leaf = true;
        } else if awaiting_leaf {
            leaves.push(leaf_symbol(line));
            awaiting_leaf = false;
        }
    }

    let total = leaves.len();
    let mut counts = std::collections::BTreeMap::<String, u64>::new();
    for symbol in leaves {
        *counts.entry(symbol).or_default() += 1;
    }
    let mut hotspots: Vec<Hotspot> = counts
        .into_iter()
        .map(|(symbol, samples)| Hotspot {
            symbol,
            samples,
            percent: percent(samples, total),
        })
        .collect();
    // Most samples first; ties keep name order so output is deterministic.
    hotspots.sort_by(|a, b| {
        b.samples
            .cmp(&a.samples)
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    hotspots
}

fn percent(part: u64, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let ratio = part as f64 / whole as f64;
    ratio * 100.0
}

/// Extracts the symbol from one frame line: drops the leading address,
/// the trailing `(object)` and the `+0x…` offset.
fn leaf_symbol(line: &str) -> String {
    let frame = line.trim();
    let after_address = frame
        .split_once(char::is_whitespace)
        .map_or(frame, |(_, rest)| rest.trim());
    let without_object = after_address
        .rsplit_once(" (")
        .filter(|(_, object)| object.ends_with(')'))
        .map_or(after_address, |(symbol, _)| symbol);
    let symbol = match without_object.rsplit_once("+0x") {
        Some((symbol, offset)) if offset.chars().all(|c| c.is_ascii_hexdigit()) => symbol,
        _ => without_object,
    };
    let symbol = symbol.trim();
    if symbol.is_empty() {
        String::from("[unknown]")
    } else {
        symbol.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "\
1234567890,,instructions,1000000,100.00,,
987654,,cycles,1000000,100.00,,
<not counted>,,branch-misses,0,0.00,,
12345,,cache-misses,1000000,100.00,,
";

    const SCRIPT: &str = "\
app 100 [000] 1.000: cycles:
\t    7f00  hot_loop::sum+0x1f (/work/target/release/app)
\t    7f10  main+0x4 (/work/target/release/app)

app 100 [000] 1.001: cycles:
\t    7f00  hot_loop::sum+0x20 (/work/target/release/app)

app 100 [000] 1.002: cycles:
\t    8000  [unknown] (/lib/libc.so.6)

app 100 [000] 1.003: cycles:
\t    7f20  core::fmt::write (/work/target/release/app)
";

    #[test]
    fn stat_args_request_repeats_and_events_before_the_command() {
        let args = stat_args(&["cycles", "instructions"], 3);
        assert_eq!(
            args,
            ["stat", "-x,", "-r", "3", "-e", "cycles,instructions", "--"]
        );
    }

    #[test]
    fn stat_parsing_reads_values_and_skips_uncounted_events() {
        let counters = parse_stat(STAT);
        let events: Vec<&str> = counters.iter().map(|c| c.event.as_str()).collect();
        assert_eq!(events, ["instructions", "cycles", "cache-misses"]);
        assert!((counters[1].value - 987_654.0).abs() < 1e-9);
    }

    #[test]
    fn stat_parsing_ignores_prose_and_blank_lines() {
        assert!(parse_stat("perf: something went wrong\n\n").is_empty());
    }

    #[test]
    fn record_and_script_arguments_are_well_formed() {
        assert_eq!(
            record_args("out.data", 0),
            ["record", "-F", "1", "-g", "-o", "out.data", "--"]
        );
        assert_eq!(script_args("out.data"), ["script", "-i", "out.data"]);
    }

    #[test]
    fn hotspots_use_the_leaf_frame_and_rank_by_samples() {
        let hotspots = parse_script(SCRIPT);
        assert_eq!(hotspots[0].symbol, "hot_loop::sum");
        assert_eq!(hotspots[0].samples, 2);
        assert!((hotspots[0].percent - 50.0).abs() < 1e-9);
        let symbols: Vec<&str> = hotspots.iter().map(|h| h.symbol.as_str()).collect();
        assert_eq!(symbols, ["hot_loop::sum", "[unknown]", "core::fmt::write"]);
    }

    #[test]
    fn leaf_is_not_the_caller_frame() {
        // `main` appears in the first sample's second frame and must not count.
        assert!(!parse_script(SCRIPT).iter().any(|h| h.symbol == "main"));
    }

    #[test]
    fn empty_script_has_no_hotspots() {
        assert!(parse_script("").is_empty());
    }
}
