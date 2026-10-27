//! Build-time analysis for `tpt-profiler` (spec §8.6, §20).
//!
//! Reads the unit timings that `cargo build --timings` writes into its HTML
//! report and turns them into a per-crate cost table, a split between
//! library, build-script and proc-macro work, and findings where that cost
//! is concentrated. The process runs that produce the report live in the
//! core crate; this crate only parses and judges what they produced.
//!
//! Unit durations are CPU-side sums over parallel jobs, not wall-clock
//! time, so shares describe where compile work goes rather than how long a
//! build takes. Feature-combination cost (spec §20) needs several builds
//! and is not covered here.

use std::collections::{BTreeMap, BTreeSet};

use tpt_profiler_model::error::ParseError;
use tpt_profiler_model::{Category, Confidence};

/// Marker that precedes the timing array in the report's JavaScript.
const UNIT_DATA_MARKER: &str = "UNIT_DATA = ";

/// Share of unit time above which build scripts are worth a finding.
pub const BUILD_SCRIPT_SHARE: f64 = 0.10;

/// Share of unit time above which proc-macro crates are worth a finding.
pub const PROC_MACRO_SHARE: f64 = 0.15;

/// Share of unit time above which a single crate is worth a finding.
pub const SINGLE_CRATE_SHARE: f64 = 0.25;

/// One compile unit as reported by `cargo build --timings`.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitTiming {
    /// Package name as cargo reports it (underscores, not hyphens).
    pub name: String,
    /// Package version.
    pub version: String,
    /// Target description. Build scripts say `build-script`; other units
    /// are empty.
    pub target: String,
    /// Duration of the unit in seconds.
    pub duration_seconds: f64,
}

/// What a compile unit spends its time on (spec §20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnitKind {
    /// Ordinary library or binary compilation.
    Library,
    /// A `build.rs` compile or run.
    BuildScript,
    /// A procedural-macro crate.
    ProcMacro,
}

impl UnitKind {
    /// Short label for reports.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Library => "library",
            Self::BuildScript => "build-script",
            Self::ProcMacro => "proc-macro",
        }
    }
}

impl UnitTiming {
    /// Classifies this unit. `proc_macros` holds the normalised names of
    /// packages whose library target is a proc-macro, as reported by cargo
    /// metadata; the timing report does not mark them itself.
    #[must_use]
    pub fn kind(&self, proc_macros: &BTreeSet<String>) -> UnitKind {
        if self.target.contains("build-script") {
            UnitKind::BuildScript
        } else if proc_macros.contains(&normalise_name(&self.name)) {
            UnitKind::ProcMacro
        } else {
            UnitKind::Library
        }
    }
}

/// Normalises a package name so hyphenated metadata names match the
/// underscored names used in timing reports (`proc-macro2` → `proc_macro2`).
#[must_use]
pub fn normalise_name(name: &str) -> String {
    name.replace('-', "_")
}

/// Parses the unit timings out of a `cargo-timing.html` report.
///
/// # Errors
/// Fails when the report has no timing array or the array is malformed.
/// The error names the line of the report where the array was expected.
pub fn parse_timing_report(html: &str) -> Result<Vec<UnitTiming>, ParseError> {
    let parse_error = |message: String, offset: usize| ParseError::InvalidSyntax {
        path: String::from("cargo-timing.html"),
        line: u32::try_from(html[..offset].matches('\n').count() + 1).unwrap_or(u32::MAX),
        message,
    };

    let marker = html
        .find(UNIT_DATA_MARKER)
        .ok_or_else(|| parse_error(String::from("no UNIT_DATA array found"), 0))?;
    let json_start = marker + UNIT_DATA_MARKER.len();
    let value = serde_json::Deserializer::from_str(&html[json_start..])
        .into_iter::<serde_json::Value>()
        .next()
        .ok_or_else(|| parse_error(String::from("UNIT_DATA is empty"), json_start))?
        .map_err(|error| parse_error(error.to_string(), json_start))?;
    let serde_json::Value::Array(units) = value else {
        return Err(parse_error(
            String::from("UNIT_DATA is not an array"),
            json_start,
        ));
    };
    units
        .iter()
        .map(|unit| {
            parse_unit(unit).ok_or_else(|| {
                parse_error(String::from("unit without a name or duration"), json_start)
            })
        })
        .collect()
}

/// Reads one entry of the unit array. Name and duration are required;
/// version and target default to empty.
fn parse_unit(unit: &serde_json::Value) -> Option<UnitTiming> {
    Some(UnitTiming {
        name: unit.get("name")?.as_str()?.to_owned(),
        version: unit
            .get("version")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        target: unit
            .get("target")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        duration_seconds: unit.get("duration")?.as_f64()?,
    })
}

/// Build cost for one crate, split by what the cost is spent on.
#[derive(Debug, Clone, PartialEq)]
pub struct CrateCost {
    /// Package name.
    pub name: String,
    /// Package version.
    pub version: String,
    /// What the cost is spent on.
    pub kind: UnitKind,
    /// Summed unit duration in seconds.
    pub seconds: f64,
    /// Fraction of all unit time, in `0.0..=1.0`.
    pub share: f64,
}

/// Aggregated build cost for one build.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BuildSummary {
    /// Summed duration of every unit, in seconds.
    pub unit_seconds: f64,
    /// Time spent on build scripts.
    pub build_script_seconds: f64,
    /// Time spent on proc-macro crates.
    pub proc_macro_seconds: f64,
    /// Time spent on ordinary library and binary compilation.
    pub library_seconds: f64,
    /// Per-crate costs, largest first. Ties sort by name then version.
    pub crates: Vec<CrateCost>,
}

impl BuildSummary {
    /// Fraction of unit time spent in units of `kind`.
    #[must_use]
    pub fn share_of(&self, kind: UnitKind) -> f64 {
        if self.unit_seconds <= 0.0 {
            return 0.0;
        }
        let seconds = match kind {
            UnitKind::Library => self.library_seconds,
            UnitKind::BuildScript => self.build_script_seconds,
            UnitKind::ProcMacro => self.proc_macro_seconds,
        };
        seconds / self.unit_seconds
    }
}

/// Totals up unit timings per crate and per kind.
#[must_use]
pub fn summarise(units: &[UnitTiming], proc_macros: &BTreeSet<String>) -> BuildSummary {
    let mut per_crate: BTreeMap<(String, String, UnitKind), f64> = BTreeMap::new();
    let mut summary = BuildSummary::default();
    for unit in units {
        let kind = unit.kind(proc_macros);
        let seconds = unit.duration_seconds.max(0.0);
        summary.unit_seconds += seconds;
        match kind {
            UnitKind::Library => summary.library_seconds += seconds,
            UnitKind::BuildScript => summary.build_script_seconds += seconds,
            UnitKind::ProcMacro => summary.proc_macro_seconds += seconds,
        }
        *per_crate
            .entry((unit.name.clone(), unit.version.clone(), kind))
            .or_default() += seconds;
    }

    let total = summary.unit_seconds;
    let mut crates: Vec<CrateCost> = per_crate
        .into_iter()
        .map(|((name, version, kind), seconds)| CrateCost {
            name,
            version,
            kind,
            seconds,
            share: if total > 0.0 { seconds / total } else { 0.0 },
        })
        .collect();
    crates.sort_by(|left, right| {
        right
            .seconds
            .total_cmp(&left.seconds)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.version.cmp(&right.version))
    });
    summary.crates = crates;
    summary
}

/// A build-cost finding (spec §20).
#[derive(Debug, Clone, PartialEq)]
pub struct BuildFinding {
    /// Category, always compile time.
    pub category: Category,
    /// Human-readable description with the recommendation.
    pub description: String,
    /// Confidence. A single timed build is at most `Low` (spec §50).
    pub confidence: Confidence,
    /// Where the cost sits, as a crate name or a unit kind.
    pub subject: String,
}

/// Judges a summary against the fixed thresholds and returns findings,
/// largest first.
#[must_use]
pub fn findings(summary: &BuildSummary) -> Vec<BuildFinding> {
    let mut found = Vec::new();
    let mut push = |subject: &str, description: String| {
        found.push(BuildFinding {
            category: Category::CompileTime,
            description,
            confidence: Confidence::Low,
            subject: subject.to_owned(),
        });
    };

    let script_share = summary.share_of(UnitKind::BuildScript);
    if script_share >= BUILD_SCRIPT_SHARE {
        push(
            "build scripts",
            format!(
                "build scripts take {:.0}% of compile-unit time ({:.1}s); check each build.rs \
                 for work that could be cached, gated or skipped (spec §20)",
                script_share * 100.0,
                summary.build_script_seconds
            ),
        );
    }

    let macro_share = summary.share_of(UnitKind::ProcMacro);
    if macro_share >= PROC_MACRO_SHARE {
        push(
            "proc-macros",
            format!(
                "proc-macro crates take {:.0}% of compile-unit time ({:.1}s); drop unused derive \
                 or attribute macros and disable default features that pull them in (spec §20)",
                macro_share * 100.0,
                summary.proc_macro_seconds
            ),
        );
    }

    for crate_cost in &summary.crates {
        if crate_cost.share >= SINGLE_CRATE_SHARE {
            push(
                &crate_cost.name,
                format!(
                    "`{} {}` takes {:.0}% of compile-unit time ({:.1}s); check whether its \
                     default features or a lighter alternative would cut that (spec §20)",
                    crate_cost.name,
                    crate_cost.version,
                    crate_cost.share * 100.0,
                    crate_cost.seconds
                ),
            );
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(name: &str, target: &str, seconds: f64) -> UnitTiming {
        UnitTiming {
            name: name.to_owned(),
            version: String::from("1.0.0"),
            target: target.to_owned(),
            duration_seconds: seconds,
        }
    }

    fn report(units: &str) -> String {
        format!(
            "<html>\n<script>\n// Index into UNIT_DATA\nlet HIT_BOXES = [];\n\
             const UNIT_DATA = {units};\nlet x = 1;\n</script>"
        )
    }

    #[test]
    fn parses_units_from_the_report_script() {
        let html = report(
            r#"[{"i":0,"name":"serde","version":"1.0.229","mode":"todo","target":"","duration":2.5},
                {"i":1,"name":"serde_json","version":"1.0.1","mode":"run-custom-build","target":" build-script (run)","duration":0.5}]"#,
        );
        let units = parse_timing_report(&html).expect("parses");
        assert_eq!(units.len(), 2);
        assert_eq!(units[0].name, "serde");
        assert_eq!(units[0].version, "1.0.229");
        assert!((units[1].duration_seconds - 0.5).abs() < 1e-9);
        assert_eq!(units[1].target, " build-script (run)");
    }

    #[test]
    fn report_without_unit_data_is_a_parse_error_with_a_line() {
        let error = parse_timing_report("<html>\n<body/>\n</html>").unwrap_err();
        assert!(matches!(error, ParseError::InvalidSyntax { .. }));
    }

    #[test]
    fn malformed_unit_is_rejected_not_skipped() {
        let html = report(r#"[{"name":"serde"}]"#);
        assert!(parse_timing_report(&html).is_err());
    }

    #[test]
    fn build_script_and_proc_macro_units_are_classified() {
        let proc_macros: BTreeSet<String> = [normalise_name("serde-derive")].into_iter().collect();
        assert_eq!(
            unit("build_script_build", " build-script", 1.0).kind(&proc_macros),
            UnitKind::BuildScript
        );
        assert_eq!(
            unit("serde_derive", "", 1.0).kind(&proc_macros),
            UnitKind::ProcMacro
        );
        assert_eq!(unit("serde", "", 1.0).kind(&proc_macros), UnitKind::Library);
    }

    #[test]
    fn summary_splits_time_and_orders_crates() {
        let units = vec![
            unit("serde", "", 2.0),
            unit("serde", "", 1.0),
            unit("syn", "", 1.0),
            unit("build", " build-script", 1.0),
        ];
        let summary = summarise(&units, &BTreeSet::new());
        assert!((summary.unit_seconds - 5.0).abs() < 1e-9);
        assert!((summary.library_seconds - 4.0).abs() < 1e-9);
        assert!((summary.share_of(UnitKind::BuildScript) - 0.2).abs() < 1e-9);
        assert_eq!(summary.crates[0].name, "serde");
        assert!((summary.crates[0].seconds - 3.0).abs() < 1e-9);
        assert!((summary.crates[0].share - 0.6).abs() < 1e-9);
    }

    #[test]
    fn empty_build_has_zero_shares_and_no_findings() {
        let summary = summarise(&[], &BTreeSet::new());
        assert!((summary.share_of(UnitKind::Library)).abs() < 1e-9);
        assert_eq!(findings(&summary).len(), 0);
    }

    #[test]
    fn concentrated_cost_produces_findings_at_low_confidence() {
        let units = vec![
            unit("huge", "", 8.0),
            unit("small", "", 1.0),
            unit("build", " build-script", 2.0),
        ];
        let summary = summarise(&units, &BTreeSet::new());
        let found = findings(&summary);
        assert!(
            found
                .iter()
                .any(|finding| finding.subject == "build scripts")
        );
        assert!(found.iter().any(|finding| finding.subject == "huge"));
        assert!(
            found
                .iter()
                .all(|finding| finding.confidence == Confidence::Low)
        );
        assert!(
            found
                .iter()
                .all(|finding| finding.category == Category::CompileTime)
        );
    }

    #[test]
    fn spread_out_cost_is_not_flagged() {
        let units: Vec<UnitTiming> = (0..10)
            .map(|index| unit(&format!("crate{index}"), "", 1.0))
            .collect();
        assert_eq!(findings(&summarise(&units, &BTreeSet::new())).len(), 0);
    }
}
