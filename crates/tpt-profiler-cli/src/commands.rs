//! Command implementations. Every command returns `Ok(())` on success
//! (printing its report) and a structured [`Error`] on failure; exit
//! codes are decided in `main.rs`.

use std::fmt;
use std::path::PathBuf;

use tpt_profiler::config::Config;
use tpt_profiler::pipeline;
use tpt_profiler_report as report;

/// Shared command context from global CLI flags.
pub struct Context {
    pub format: Format,
    pub output: Option<PathBuf>,
    pub config_path: Option<PathBuf>,
    pub path: PathBuf,
    pub quiet: bool,
    pub verbose: bool,
}

/// Output formats (spec §42).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Terminal,
    Json,
    Markdown,
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
        match &self.config_path {
            Some(path) => Ok(Config::load(path)?),
            None => Ok(Config::load_discovered(&self.path)?.0),
        }
    }

    /// Colour is safe only when NO_COLOR is unset (spec §42); tty
    /// detection is left to the terminal, ANSI is harmless without one.
    fn should_colour(&self) -> bool {
        !self.quiet && std::env::var_os("NO_COLOR").is_none()
    }

    fn emit(&self, text: &str) -> Result<(), Error> {
        match &self.output {
            Some(path) => {
                std::fs::write(path, text).map_err(|error| {
                    Error::Message(format!("could not write {}: {error}", path.display()))
                })?;
                if !self.quiet {
                    eprintln!("written to {}", path.display());
                }
                Ok(())
            }
            None => {
                println!("{text}");
                Ok(())
            }
        }
    }
}

/// `scan` — project structure tree (spec §7).
pub fn scan(context: &Context) -> CommandResult {
    let (repository, _root) = tpt_profiler::discovery::discover_repository(&context.path)?;
    let text = report::render_scan(&repository);
    if context.format == Format::Json {
        let json = serde_json::to_string_pretty(&repository)?;
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
            let json = serde_json::to_string_pretty(&run.repository)?;
            context.emit(&json)
        }
        Format::Markdown => {
            let markdown = report::render_markdown(&run.repository.name, &run.opportunities);
            context.emit(&markdown)
        }
    }
}

/// `runtime` — execute configured workloads (spec §8.2).
pub fn runtime(context: &Context) -> CommandResult {
    let config = context.load_config()?;
    let environment = tpt_profiler::environment::capture_environment(Some(&context.path));
    let mut lines = Vec::new();
    for workload in &config.workloads {
        let mut runnable = tpt_profiler_runtime::RunnableWorkload::new(
            tpt_profiler_model::Workload {
                name: workload.name.clone(),
                input: None,
                duration_hint_ms: None,
                repetitions: workload.repetitions,
            },
            &context.path.join(&workload.command),
        );
        runnable.args = workload.args.clone();
        if let Some(warmup) = workload.warmup_runs {
            runnable.warmup_runs = warmup;
        }
        if let Some(timeout_ms) = workload.timeout_ms {
            runnable.timeout = std::time::Duration::from_millis(timeout_ms);
        }
        let result = tpt_profiler_runtime::measure_workload(&runnable, None)?;
        for measurement in tpt_profiler_runtime::to_measurements(
            &result,
            tpt_profiler_model::MeasurementSource::WallClock,
            now_unix(),
            Some(&environment),
        ) {
            lines.push(format!(
                "{}: {:.4} {} (confidence {}, n={})",
                measurement.metric,
                measurement.value,
                measurement.unit,
                measurement.confidence,
                result.summary.n,
            ));
        }
        if result.failed_runs > 0 {
            lines.push(format!(
                "{}: {} run(s) failed or timed out",
                workload.name, result.failed_runs
            ));
        }
    }
    if lines.is_empty() {
        context.emit("No workloads configured — add a [[workloads]] section (spec §23).")
    } else {
        context.emit(&lines.join("\n"))
    }
}

/// `bench` — run benchmarks and ingest Criterion (spec §8.3).
pub fn bench(context: &Context) -> CommandResult {
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
        tpt_profiler_model::MeasurementSource::Criterion,
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

/// `baseline <name>` — save current measurements (spec §36).
pub fn baseline(context: &Context, name: &str) -> CommandResult {
    let config = context.load_config()?;
    let run = pipeline::analyse(&context.path, &config)?;
    let environment = tpt_profiler::environment::capture_environment(Some(&context.path));
    let mut profile = pipeline::to_profile_run(&run, name, now_unix(), environment.clone());
    profile.environment = environment;
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
    let environment = tpt_profiler::environment::capture_environment(Some(&context.path));
    let profile = pipeline::to_profile_run(&run, name, now_unix(), environment);
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
    if run.no_opportunities {
        return context.emit("No opportunities detected.");
    }
    let mut opportunities = run.opportunities.clone();
    tpt_profiler_opportunity::sort_by_strategy(&mut opportunities, strategy);
    opportunities.truncate(top);
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
            let json = serde_json::to_string_pretty(&run.repository)?;
            context.emit(&json)
        }
        Format::Markdown => {
            let markdown = report::render_markdown(&run.repository.name, &opportunities);
            context.emit(&markdown)
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
        .map_or(0, |duration| duration.as_secs() as i64)
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
