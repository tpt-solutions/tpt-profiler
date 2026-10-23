//! Command implementations. Every command returns `Ok(())` on success
//! (printing its report) and a structured [`Error`] on failure; exit
//! codes are decided in `main.rs`.

use std::fmt;
use std::path::PathBuf;

use tpt_profiler::config::{Config, Overrides};
use tpt_profiler::pipeline;
use tpt_profiler_model::SchemaEnvelope;
use tpt_profiler_report as report;

/// Shared command context from global CLI flags.
pub struct Context {
    pub format: Format,
    pub output: Option<PathBuf>,
    /// File to append a Markdown summary to (spec §39), if requested.
    pub summary: Option<PathBuf>,
    pub config_path: Option<PathBuf>,
    pub path: PathBuf,
    pub quiet: bool,
    pub verbose: bool,
    /// Modes turned off by `--disable` (spec §40).
    pub overrides: Overrides,
}

/// Output formats (spec §42).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Terminal,
    Json,
    Jsonl,
    Markdown,
    Html,
    GithubAnnotations,
}

/// Command failure (spec §74); rendered by `main` and mapped to a
/// non-zero exit.
#[derive(Debug)]
pub enum Error {
    Model(tpt_profiler_model::Error),
    Message(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Model(error) => write!(f, "{error}"),
            Self::Message(message) => write!(f, "{message}"),
        }
    }
}

impl From<tpt_profiler_model::Error> for Error {
    fn from(error: tpt_profiler_model::Error) -> Self {
        Self::Model(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Model(tpt_profiler_model::error::Error::Serialization(
            tpt_profiler_model::error::SerializationError::EncodeFailed {
                message: error.to_string(),
            },
        ))
    }
}

type CommandResult = Result<(), Error>;

impl Context {
    /// Loads the configuration: explicit `--config` wins, otherwise
    /// discovery from `--path` (flags > config > defaults, spec §40).
    fn load_config(&self) -> Result<Config, Error> {
        let mut config = match &self.config_path {
            Some(path) => Config::load(path)?,
            None => Config::load_discovered(&self.path)?.0,
        };
        config.apply_overrides(&self.overrides);
        Ok(config)
    }

    /// Colour is safe only when NO_COLOR is unset (spec §42); tty
    /// detection is left to the terminal, ANSI is harmless without one.
    fn should_colour(&self) -> bool {
        !self.quiet && std::env::var_os("NO_COLOR").is_none()
    }

    fn emit(&self, text: &str) -> Result<(), Error> {
        let Some(path) = &self.output else {
            println!("{text}");
            return Ok(());
        };
        std::fs::write(path, text).map_err(|error| {
            Error::Message(format!("could not write {}: {error}", path.display()))
        })?;
        if !self.quiet {
            eprintln!("written to {}", path.display());
        }
        Ok(())
    }
}

/// Schema-versioned generator name for JSON output (spec §69).
fn generated_by() -> String {
    format!("tpt-profiler {}", tpt_profiler::version())
}

/// `scan` — project structure tree (spec §7).
pub fn scan(context: &Context) -> CommandResult {
    let (repository, _root) = tpt_profiler::discovery::discover_repository(&context.path)?;
    let text = report::render_scan(&repository);
    if context.format == Format::Json {
        let json = serde_json::to_string_pretty(&SchemaEnvelope::new(generated_by(), &repository))?;
        context.emit(&json)
    } else {
        context.emit(&text)
    }
}

/// `static` — static analysis + opportunities (spec §8.1).
pub fn static_analysis(context: &Context) -> CommandResult {
    let config = context.load_config()?;
    let run = pipeline::analyse(&context.path, &config)?;
    emit_analysis(context, &run)
}

/// `full` — every applicable analysis (spec §8.7). MVP scope: the
/// static-side pipeline (runtime/bench run through their own commands,
/// gated by their config toggles in later phases).
pub fn full(context: &Context) -> CommandResult {
    let config = context.load_config()?;
    let run = pipeline::analyse(&context.path, &config)?;
    if !context.quiet {
        let tree = report::render_scan(&run.repository);
        eprintln!("{tree}");
    }
    emit_analysis(context, &run)
}

/// Appends Markdown to the `--summary` file, when one was given (spec §39).
/// Appending keeps several commands in one CI job summary.
fn append_summary(context: &Context, markdown: &str) -> CommandResult {
    use std::io::Write as _;

    let Some(path) = &context.summary else {
        return Ok(());
    };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| Error::Message(format!("could not open {}: {error}", path.display())))?;
    file.write_all(markdown.as_bytes())
        .map_err(|error| Error::Message(format!("could not write {}: {error}", path.display())))
}

/// Appends the opportunity report to the `--summary` file.
fn write_summary(
    context: &Context,
    repository: &str,
    opportunities: &[tpt_profiler_model::Opportunity],
) -> CommandResult {
    if context.summary.is_none() {
        return Ok(());
    }
    append_summary(context, &report::render_markdown(repository, opportunities))
}

fn emit_analysis(context: &Context, run: &pipeline::AnalysisRun) -> CommandResult {
    if !context.quiet {
        for error in &run.static_report.parse_errors {
            eprintln!("warning: {error}");
        }
    }
    if context.verbose {
        eprintln!(
            "analysed {} file(s); {} parse error(s); {} dependency finding(s)",
            run.static_report.files_analyzed,
            run.static_report.parse_errors.len(),
            run.dependency_report.findings.len(),
        );
    }
    write_summary(context, &run.repository.name, &run.opportunities)?;
    match context.format {
        Format::Terminal => {
            let text = report::render_opportunities(
                &run.repository.name,
                &run.opportunities,
                &report::TerminalOptions {
                    colour: context.should_colour(),
                    width: terminal_width(),
                },
            );
            context.emit(&text)
        }
        Format::Json => {
            let json = report::render_opportunities_json(
                &generated_by(),
                &run.repository.name,
                &run.opportunities,
            )?;
            context.emit(&json)
        }
        Format::Jsonl => {
            let jsonl =
                report::render_opportunities_jsonl(&run.repository.name, &run.opportunities)?;
            context.emit(&jsonl)
        }
        Format::Markdown => {
            let markdown = report::render_markdown(&run.repository.name, &run.opportunities);
            context.emit(&markdown)
        }
        Format::Html => {
            let html = report::render_html(&run.repository.name, &run.opportunities);
            context.emit(&html)
        }
        Format::GithubAnnotations => {
            let annotations = report::render_github_annotations(&run.opportunities);
            context.emit(&annotations)
        }
    }
}

/// `runtime` — execute configured workloads (spec §8.2).
pub fn runtime(context: &Context) -> CommandResult {
    let config = context.load_config()?;
    if !config.profiling.runtime {
        return context.emit(
            "runtime profiling is disabled (profiling.runtime = false or --disable runtime)",
        );
    }
    let environment = tpt_profiler::environment::capture_environment(Some(&context.path));
    let outcomes =
        tpt_profiler::measure::collect_runtime(&context.path, &config, &environment, now_unix())?;
    let mut lines = Vec::new();
    for outcome in &outcomes {
        for measurement in &outcome.measurements {
            lines.push(format!(
                "{}: {:.4} {} (confidence {}, n={})",
                measurement.metric,
                measurement.value,
                measurement.unit,
                measurement.confidence,
                outcome.samples,
            ));
        }
        if outcome.failed_runs > 0 {
            lines.push(format!(
                "{}: {} run(s) failed or timed out",
                outcome.name, outcome.failed_runs
            ));
        }
    }
    if outcomes.is_empty() {
        context.emit("No workloads configured — add a [[workloads]] section (spec §23).")
    } else {
        context.emit(&lines.join("\n"))
    }
}

/// `bench` — run benchmarks and ingest Criterion (spec §8.3).
pub fn bench(context: &Context) -> CommandResult {
    if !context.load_config()?.benchmarks.enabled {
        return context.emit(
            "benchmarks are disabled ([benchmarks] enabled = false or --disable benchmarks)",
        );
    }
    let (repository, workspace_root) = tpt_profiler::discovery::discover_repository(&context.path)?;
    let infos: Vec<tpt_profiler_model::hierarchy::CrateInfo> = repository
        .projects
        .iter()
        .flat_map(|project| project.crates.clone())
        .collect();
    let bench_report = tpt_profiler_bench::run_benches(
        &workspace_root,
        &infos,
        std::time::Duration::from_secs(1800),
    )?;
    if bench_report.no_benchmarks {
        return context.emit("No benchmarks found (spec §8.3: this is a result, not a failure).");
    }
    let measurements = tpt_profiler_bench::to_measurements(
        &bench_report.estimates,
        &tpt_profiler_model::MeasurementSource::Criterion,
        now_unix(),
    );
    let lines: Vec<String> = measurements
        .iter()
        .map(|measurement| {
            format!(
                "{}: {:.6} s (criterion)",
                measurement.metric, measurement.value
            )
        })
        .collect();
    if lines.is_empty() {
        context.emit("Benchmarks ran but no Criterion data was produced.")
    } else {
        context.emit(&lines.join("\n"))
    }
}

/// Builds the run record for `baseline`, `compare` and `ci`: static
/// findings plus the runtime measurements from configured workloads
/// (spec §8.2, §36). The workload outcomes are returned too, so callers can
/// report failed runs.
fn measured_profile(
    context: &Context,
    config: &Config,
    run: &pipeline::AnalysisRun,
    name: &str,
) -> Result<
    (
        tpt_profiler_model::ProfileRun,
        Vec<tpt_profiler::measure::WorkloadOutcome>,
    ),
    Error,
> {
    let timestamp = now_unix();
    let environment = tpt_profiler::environment::capture_environment(Some(&context.path));
    let outcomes =
        tpt_profiler::measure::collect_runtime(&context.path, config, &environment, timestamp)?;
    let mut profile = pipeline::to_profile_run(run, name, timestamp, environment);
    for outcome in &outcomes {
        profile
            .measurements
            .extend(outcome.measurements.iter().cloned());
    }
    Ok((profile, outcomes))
}

/// `baseline <name>` — save current measurements (spec §36).
pub fn baseline(context: &Context, name: &str) -> CommandResult {
    let config = context.load_config()?;
    let run = pipeline::analyse(&context.path, &config)?;
    let (profile, _) = measured_profile(context, &config, &run, name)?;
    let stored = tpt_profiler::baseline::baseline_from_run(name, &profile);
    let path = tpt_profiler::baseline::save_baseline(&run.workspace_root, &stored)?;
    if !context.quiet {
        println!(
            "baseline `{}` saved with {} measurement(s) at {}",
            name,
            stored.measurements.len(),
            path.display()
        );
    }
    Ok(())
}

/// `compare <name>` — compare against a baseline (spec §36).
pub fn compare(context: &Context, name: &str) -> CommandResult {
    let config = context.load_config()?;
    let run = pipeline::analyse(&context.path, &config)?;
    let Some(baseline) = tpt_profiler::baseline::load_baseline(&run.workspace_root, name)? else {
        return Err(Error::Message(format!(
            "baseline `{name}` not found — run `tpt-profiler baseline {name}` first"
        )));
    };
    let (profile, _) = measured_profile(context, &config, &run, name)?;
    // Comparable measurements come from a runtime/bench run; a purely
    // static run has none, and the comparison then reports no metrics.
    let comparison = tpt_profiler::baseline::compare(&baseline, &profile, &config);
    let mut lines = vec![format!("Performance change vs `{name}`")];
    if let Some(warning) = &comparison.environment_warning {
        lines.push(format!("warning: {warning}"));
    }
    if comparison.metrics.is_empty() {
        lines.push(String::from(
            "no comparable measurements (run `runtime` or `bench` first)",
        ));
    }
    for metric in &comparison.metrics {
        lines.push(format!(
            "  {}: {} {} (baseline {:.4} → current {:.4})",
            metric.metric,
            metric.change_percent(),
            match metric.assessment {
                tpt_profiler::baseline::Assessment::Improved => "improved",
                tpt_profiler::baseline::Assessment::Regressed => "REGRESSED",
                tpt_profiler::baseline::Assessment::Unchanged => "unchanged",
                tpt_profiler::baseline::Assessment::Noisy => "noisy — not judged",
            },
            metric.baseline,
            metric.current,
        ));
    }
    context.emit(&lines.join("\n"))
}

/// `ci <name>` — apply the `[ci]` policy to the comparison against a
/// baseline (spec §39). Exits non-zero when the policy fails the run.
pub fn ci(context: &Context, name: &str) -> CommandResult {
    use std::fmt::Write as _;

    let config = context.load_config()?;
    if !config.ci.enabled {
        return context.emit("CI checks are disabled ([ci] enabled = false)");
    }
    let run = pipeline::analyse(&context.path, &config)?;
    let Some(baseline) = tpt_profiler::baseline::load_baseline(&run.workspace_root, name)? else {
        return Err(Error::Message(format!(
            "baseline `{name}` not found — run `tpt-profiler baseline {name}` on the reference branch first"
        )));
    };
    let (profile, workloads) = measured_profile(context, &config, &run, name)?;
    let comparison = tpt_profiler::baseline::compare(&baseline, &profile, &config);
    let mut outcomes = tpt_profiler::ci::evaluate(&comparison, &config.ci);
    // A workload that failed or timed out yields no measurements, so the
    // comparison cannot cover it; say so rather than letting it pass quietly.
    for workload in workloads.iter().filter(|workload| workload.failed_runs > 0) {
        outcomes.push(tpt_profiler::ci::Outcome {
            severity: tpt_profiler::ci::Severity::Warn,
            message: format!(
                "workload `{}`: {} run(s) failed or timed out",
                workload.name, workload.failed_runs
            ),
        });
    }

    let text = if context.format == Format::GithubAnnotations {
        outcomes
            .iter()
            .map(|outcome| {
                let command = match outcome.severity {
                    tpt_profiler::ci::Severity::Fail => "error",
                    tpt_profiler::ci::Severity::Warn => "warning",
                    tpt_profiler::ci::Severity::Info => "notice",
                };
                format!("::{command}::{}", outcome.message)
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        let mut lines = vec![format!("CI policy vs `{name}`")];
        if outcomes.is_empty() {
            lines.push(String::from("  passed: no regressions"));
        }
        for outcome in &outcomes {
            let label = match outcome.severity {
                tpt_profiler::ci::Severity::Fail => "FAIL",
                tpt_profiler::ci::Severity::Warn => "warn",
                tpt_profiler::ci::Severity::Info => "info",
            };
            lines.push(format!("  [{label}] {}", outcome.message));
        }
        lines.join("\n")
    };
    context.emit(&text)?;

    let mut markdown = format!("## CI policy vs `{name}`\n\n");
    if outcomes.is_empty() {
        markdown.push_str("No regressions.\n");
    }
    for outcome in &outcomes {
        let label = match outcome.severity {
            tpt_profiler::ci::Severity::Fail => "FAIL",
            tpt_profiler::ci::Severity::Warn => "warn",
            tpt_profiler::ci::Severity::Info => "info",
        };
        let _ = writeln!(markdown, "- **{label}** {}", outcome.message);
    }
    append_summary(context, &markdown)?;

    if tpt_profiler::ci::failed(&outcomes) {
        return Err(Error::Message(format!(
            "CI policy failed against `{name}` (see FAIL lines above)"
        )));
    }
    Ok(())
}

/// `report <name>` — print stored results (spec §42). `latest` re-runs
/// the analysis in-process; stored run persistence lands with
/// opportunity tracking (Phase 3).
pub fn report(context: &Context, name: &str) -> CommandResult {
    if name != "latest" {
        return Err(Error::Message(format!(
            "stored report `{name}` not found — only `latest` is available in this version"
        )));
    }
    static_analysis(context)
}

/// `opportunities --strategy --top` (spec §41, §59).
pub fn opportunities(
    context: &Context,
    strategy: tpt_profiler_opportunity::Strategy,
    top: usize,
) -> CommandResult {
    let config = context.load_config()?;
    let run = pipeline::analyse(&context.path, &config)?;
    let mut opportunities = run.opportunities.clone();
    tpt_profiler_opportunity::sort_by_strategy(&mut opportunities, strategy);
    opportunities.truncate(top);
    write_summary(context, &run.repository.name, &opportunities)?;
    if run.no_opportunities && context.format == Format::Terminal {
        return context.emit("No opportunities detected.");
    }
    match context.format {
        Format::Terminal => {
            let text = report::render_opportunities(
                &run.repository.name,
                &opportunities,
                &report::TerminalOptions {
                    colour: context.should_colour(),
                    width: terminal_width(),
                },
            );
            context.emit(&text)
        }
        Format::Json => {
            let json = report::render_opportunities_json(
                &generated_by(),
                &run.repository.name,
                &opportunities,
            )?;
            context.emit(&json)
        }
        Format::Jsonl => {
            let jsonl = report::render_opportunities_jsonl(&run.repository.name, &opportunities)?;
            context.emit(&jsonl)
        }
        Format::Markdown => {
            let markdown = report::render_markdown(&run.repository.name, &opportunities);
            context.emit(&markdown)
        }
        Format::Html => {
            let html = report::render_html(&run.repository.name, &opportunities);
            context.emit(&html)
        }
        Format::GithubAnnotations => {
            let annotations = report::render_github_annotations(&opportunities);
            context.emit(&annotations)
        }
    }
}

/// `explain <id>` — full reasoning for one opportunity (spec §41).
pub fn explain(context: &Context, id: &str) -> CommandResult {
    let config = context.load_config()?;
    let run = pipeline::analyse(&context.path, &config)?;
    let match_text = run
        .opportunities
        .iter()
        .find(|opportunity| opportunity.id.as_str().starts_with(id))
        .map(tpt_profiler_opportunity::explain);
    match match_text {
        Some(text) => context.emit(&text),
        None => Err(Error::Message(format!(
            "no opportunity with id starting `{id}` in the latest analysis"
        ))),
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

/// Terminal width: `COLUMNS` when set, else the 100-column default
/// (spec §42: width-aware output).
fn terminal_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|columns| columns.parse().ok())
        .filter(|width| *width >= 40)
        .unwrap_or(100)
}
