//! Command implementations. Every command returns `Ok(())` on success
//! (printing its report) and a structured [`Error`] on failure; exit
//! codes are decided in `main.rs`.

use std::collections::BTreeMap;
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
/// gated by their config toggles in later phases). With `save`, the run is
/// also stored under that name for `report <name>` (spec §42).
pub fn full(context: &Context, save: Option<&str>) -> CommandResult {
    let config = context.load_config()?;
    let run = pipeline::analyse(&context.path, &config)?;
    if !context.quiet {
        let tree = report::render_scan(&run.repository);
        eprintln!("{tree}");
    }
    if let Some(name) = save {
        let timestamp = now_unix();
        let environment = tpt_profiler::environment::capture_environment(Some(&context.path));
        let profile = pipeline::to_profile_run(&run, name, timestamp, environment);
        let path = tpt_profiler::runs::save_run(&run.workspace_root, name, &profile)?;
        if !context.quiet {
            eprintln!("run `{name}` saved at {}", path.display());
        }
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
            "analysed {} file(s) ({} from cache); {} parse error(s); {} dependency finding(s)",
            run.static_report.files_analyzed,
            run.static_report.cache_hits,
            run.static_report.parse_errors.len(),
            run.dependency_report.findings.len(),
        );
    }
    write_summary(context, &run.repository.name, &run.opportunities)?;
    emit_opportunities(context, &run.repository.name, &run.opportunities)
}

/// Writes opportunities in the requested format (spec §42). Shared by live
/// analyses and stored runs, so every source renders the same way.
fn emit_opportunities(
    context: &Context,
    repository: &str,
    opportunities: &[tpt_profiler_model::Opportunity],
) -> CommandResult {
    match context.format {
        Format::Terminal => {
            let text = report::render_opportunities(
                repository,
                opportunities,
                &report::TerminalOptions {
                    colour: context.should_colour(),
                    width: terminal_width(),
                },
            );
            context.emit(&text)
        }
        Format::Json => {
            let json =
                report::render_opportunities_json(&generated_by(), repository, opportunities)?;
            context.emit(&json)
        }
        Format::Jsonl => {
            let jsonl = report::render_opportunities_jsonl(repository, opportunities)?;
            context.emit(&jsonl)
        }
        Format::Markdown => {
            let markdown = report::render_markdown(repository, opportunities);
            context.emit(&markdown)
        }
        Format::Html => {
            let html = report::render_html(repository, opportunities);
            context.emit(&html)
        }
        Format::GithubAnnotations => {
            let annotations = report::render_github_annotations(opportunities);
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
    let config = context.load_config()?;
    let Some(outcome) = tpt_profiler::measure::collect_benches(&context.path, &config, now_unix())?
    else {
        return context.emit(
            "benchmarks are disabled ([benchmarks] enabled = false or --disable benchmarks)",
        );
    };
    if outcome.no_benchmarks {
        return context.emit("No benchmarks found (spec §8.3: this is a result, not a failure).");
    }
    let lines: Vec<String> = outcome
        .measurements
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
/// findings plus runtime measurements from configured workloads and bench
/// measurements from Criterion (spec §8.2, §8.3, §36). The workload outcomes
/// are returned too, so callers can report failed runs.
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
    if let Some(benches) = tpt_profiler::measure::collect_benches(&context.path, config, timestamp)?
    {
        profile.measurements.extend(benches.measurements);
    }
    profile
        .measurements
        .extend(tpt_profiler::measure::collect_binaries(
            &context.path,
            config,
            timestamp,
        )?);
    Ok((profile, outcomes))
}

/// `ecosystem [root]` — scan many repositories and report what they share
/// (spec §33, §34, §60). Static and local only; a broken repository is
/// listed with its error and the rest still run.
pub fn ecosystem(
    context: &Context,
    root: &std::path::Path,
    options: &tpt_profiler::ecosystem::EcosystemOptions,
    top: usize,
) -> CommandResult {
    let scan = tpt_profiler::ecosystem::scan(root, options);
    if context.format == Format::Json {
        let envelope = SchemaEnvelope::new(generated_by(), &scan);
        let json = serde_json::to_string_pretty(&envelope)
            .map_err(|error| Error::Message(error.to_string()))?;
        return context.emit(&json);
    }

    let text = render_ecosystem(&scan, top);
    context.emit(&text)
}

/// Options for `ecosystem --github` (spec §70, Phase 2.8a).
pub struct GithubArgs {
    /// Owner whose repositories are listed.
    pub owner: String,
    /// Include globs; empty means every Rust repository.
    pub include: Vec<String>,
    /// Exclude globs.
    pub exclude: Vec<String>,
    /// Most repositories to clone after filtering.
    pub max_repos: usize,
    /// Most kilobytes of clones on disk at once.
    pub disk_budget_kb: u64,
    /// Clone directory; the default is under `.tpt-profiler/`.
    pub cache_dir: Option<PathBuf>,
    /// Cleanup policy from `--yes` / `--keep`, or `Ask` by default.
    pub cleanup: tpt_profiler::github::CleanupPolicy,
    /// Report stored results only, with no network.
    pub from_store: bool,
    /// Score bonus per extra repository sharing an opportunity.
    pub shared_weight: f64,
    /// Rows shown in each table.
    pub top: usize,
}

/// `ecosystem --github OWNER` — lists, clones, analyses and stores an
/// owner's Rust repositories, then reports across them (spec §33, §34, §70).
pub fn ecosystem_github(context: &Context, args: &GithubArgs) -> CommandResult {
    use std::io::IsTerminal as _;
    use tpt_profiler::github::{
        self, CleanupPolicy, GitHost, GithubOptions, GithubRun, RepoFilter,
    };

    let run = if args.from_store {
        let mut repos = github::load_owner_results(&context.path, &args.owner)?;
        // Keep the newest stored commit per repository; older ones are history.
        repos.sort_by(|a, b| {
            a.name_with_owner
                .cmp(&b.name_with_owner)
                .then(b.analysed_at.cmp(&a.analysed_at))
        });
        repos.dedup_by(|a, b| a.name_with_owner == b.name_with_owner);
        GithubRun {
            repos,
            ..GithubRun::default()
        }
    } else {
        let interactive = std::io::stdin().is_terminal();
        let cleanup = match args.cleanup {
            CleanupPolicy::Ask if !interactive => CleanupPolicy::Keep,
            other => other,
        };
        let cache_root = args.cache_dir.clone().unwrap_or_else(|| {
            context
                .path
                .join(tpt_profiler::baseline::STATE_DIR)
                .join(github::CLONE_DIR)
        });
        let options = GithubOptions {
            owner: args.owner.clone(),
            filter: RepoFilter {
                include: args.include.clone(),
                exclude: args.exclude.clone(),
            },
            max_repos: args.max_repos,
            store_root: context.path.clone(),
            cache_root,
            disk_budget_kb: args.disk_budget_kb,
            workers: tpt_profiler::ecosystem::default_workers(),
            cleanup,
            confirm: &confirm_delete,
        };
        github::run(&GitHost, &options).map_err(Error::Message)?
    };

    let findings = run.repos.iter().map(|repo| {
        let opportunities = match &repo.outcome {
            github::Outcome::Analysed { opportunities } => opportunities
                .iter()
                .map(|o| (o.category.clone(), o.description.clone(), o.priority))
                .collect(),
            github::Outcome::NoManifest => Vec::new(),
        };
        (repo.name_with_owner.clone(), opportunities)
    });
    let shared = tpt_profiler::ecosystem::shared_from(findings, args.shared_weight);

    if context.format == Format::Json {
        let payload = serde_json::json!({
            "owner": args.owner,
            "repositories": run.repos,
            "skipped": run
                .skipped
                .iter()
                .map(|(name, reason)| serde_json::json!({
                    "repository": name,
                    "reason": reason.label(),
                }))
                .collect::<Vec<_>>(),
            "failures": run
                .failures
                .iter()
                .map(|(name, error)| serde_json::json!({
                    "repository": name,
                    "error": error,
                }))
                .collect::<Vec<_>>(),
            "resumed": run.resumed,
            "stopped_early": run.stopped_early,
            "shared": shared,
        });
        let envelope = SchemaEnvelope::new(generated_by(), payload);
        let json = serde_json::to_string_pretty(&envelope)
            .map_err(|error| Error::Message(error.to_string()))?;
        return context.emit(&json);
    }
    context.emit(&render_github(&args.owner, &run, &shared, args.top))
}

/// Asks before deleting clones. Reads one line from stdin; anything other
/// than `y` or `yes` keeps them.
fn confirm_delete(count: usize, kilobytes: u64) -> bool {
    use std::io::Write as _;
    eprint!(
        "Delete {count} clone(s) using {} MB? [y/N] ",
        kilobytes / 1024
    );
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim(), "y" | "Y" | "yes" | "YES")
}

/// Terminal view of a GitHub run: what was analysed, skipped and failed,
/// what the clones cost, and what is shared across the repositories.
fn render_github(
    owner: &str,
    run: &tpt_profiler::github::GithubRun,
    shared: &[tpt_profiler::ecosystem::SharedOpportunity],
    top: usize,
) -> String {
    use std::fmt::Write as _;
    use tpt_profiler::github::Outcome;

    let analysed = run
        .repos
        .iter()
        .filter(|repo| matches!(repo.outcome, Outcome::Analysed { .. }))
        .count();
    let mut text = String::new();
    let _ = writeln!(
        text,
        "GitHub owner `{owner}`: {} stored result(s), {analysed} analysed, {} reused from the store, {} skipped, {} failed",
        run.repos.len(),
        run.resumed,
        run.skipped.len(),
        run.failures.len(),
    );

    if !run.repos.is_empty() {
        let _ = writeln!(
            text,
            "\nRepositories (commit, opportunities, top priority):"
        );
        for repo in &run.repos {
            let (count, top_priority) = match &repo.outcome {
                Outcome::Analysed { opportunities } => (
                    opportunities.len().to_string(),
                    opportunities
                        .iter()
                        .filter_map(|o| o.priority)
                        .max()
                        .map_or_else(|| String::from("-"), |p| p.to_string()),
                ),
                Outcome::NoManifest => (String::from("no Cargo.toml"), String::from("-")),
            };
            let short: String = repo.commit.chars().take(10).collect();
            let _ = writeln!(
                text,
                "  {:<40} {short}  {count:>14}  {top_priority:>3}",
                repo.name_with_owner
            );
        }
    }

    if !run.failures.is_empty() {
        let _ = writeln!(text, "\nFailed (not stored; retried next run):");
        for (name, error) in &run.failures {
            let _ = writeln!(text, "  {name}: {error}");
        }
    }

    if !run.skipped.is_empty() {
        let mut reasons: BTreeMap<&str, usize> = BTreeMap::new();
        for (_, reason) in &run.skipped {
            *reasons.entry(reason.label()).or_default() += 1;
        }
        let summary: Vec<String> = reasons
            .iter()
            .map(|(reason, count)| format!("{count} {reason}"))
            .collect();
        let _ = writeln!(text, "\nSkipped: {}", summary.join(", "));
    }

    if !shared.is_empty() {
        let _ = writeln!(text, "\nShared across repositories (top {top}):");
        for entry in shared.iter().take(top) {
            let _ = writeln!(
                text,
                "  [{}] {} in {} repositories: {}",
                entry.category,
                entry.description,
                entry.repositories.len(),
                entry.suggestion
            );
        }
    }

    if let Some(reason) = &run.stopped_early {
        let _ = writeln!(text, "\nStopped early: {reason}");
    }
    if !run.remaining_clones.is_empty() {
        let kilobytes: u64 = run.remaining_clones.iter().map(|(_, size)| size).sum();
        let _ = writeln!(
            text,
            "\n{} clone(s) kept on disk ({} MB); delete them with --yes on a later run",
            run.remaining_clones.len(),
            kilobytes / 1024
        );
    }
    text.trim_end().to_owned()
}

/// The terminal view of an ecosystem scan: repositories by priority, the failures,
/// and shared opportunities with suggestions and a per-category count.
fn render_ecosystem(scan: &tpt_profiler::ecosystem::EcosystemReport, top: usize) -> String {
    use std::fmt::Write as _;

    let failed = scan
        .repositories
        .iter()
        .filter(|repo| repo.error.is_some())
        .count();
    let mut text = String::new();
    let _ = writeln!(
        text,
        "Ecosystem scan of {}: {} repositor{} ({failed} failed)",
        scan.root.display(),
        scan.repositories.len(),
        if scan.repositories.len() == 1 {
            "y"
        } else {
            "ies"
        },
    );

    let mut ranked: Vec<_> = scan
        .repositories
        .iter()
        .filter(|repo| repo.error.is_none())
        .collect();
    ranked.sort_by(|a, b| {
        b.top_priority
            .cmp(&a.top_priority)
            .then_with(|| a.name.cmp(&b.name))
    });
    if !ranked.is_empty() {
        let _ = writeln!(text, "\nRepositories (top {top} by priority):");
        let _ = writeln!(
            text,
            "  {:<32} {:>6} {:>8}",
            "repository", "found", "priority"
        );
        for repo in ranked.iter().take(top) {
            let priority = repo
                .top_priority
                .map_or_else(|| String::from("-"), |value| value.to_string());
            let _ = writeln!(
                text,
                "  {:<32} {:>6} {:>8}",
                truncate(&repo.name, 32),
                repo.opportunities,
                priority
            );
        }
    }

    for repo in scan
        .repositories
        .iter()
        .filter(|repo| repo.error.is_some())
        .take(top)
    {
        let _ = writeln!(
            text,
            "  failed: {} — {}",
            repo.name,
            repo.error.as_deref().unwrap_or_default()
        );
    }

    if scan.shared.is_empty() {
        let _ = writeln!(
            text,
            "\nNo opportunity is shared by two or more repositories."
        );
    } else {
        let _ = writeln!(
            text,
            "\nShared opportunities (in 2+ repositories, top {top} by score):"
        );
        for (index, shared) in scan.shared.iter().take(top).enumerate() {
            let _ = writeln!(
                text,
                "  {:>6.1}  {} repos  {}: {}",
                shared.score,
                shared.repositories.len(),
                shared.category,
                truncate(&shared.description, 70),
            );
            if index < 3 {
                let _ = writeln!(text, "          -> {}", shared.suggestion);
            }
        }
        let mut by_category = BTreeMap::<&str, usize>::new();
        for shared in &scan.shared {
            *by_category.entry(shared.category.as_str()).or_default() += 1;
        }
        let summary = by_category
            .iter()
            .map(|(category, count)| format!("{category} {count}"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(text, "\nShared by category: {summary}");
    }
    text.trim_end().to_owned()
}

/// Shortens `text` to `max` characters, marking the cut.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}…")
}

/// One unit kind's share of compile time, for the JSON build report.
fn build_kind_json(
    summary: &tpt_profiler_build::BuildSummary,
    kind: tpt_profiler_build::UnitKind,
) -> serde_json::Value {
    serde_json::json!({
        "kind": kind.label(),
        "seconds": summary.share_of(kind) * summary.unit_seconds,
        "share": summary.share_of(kind),
    })
}

/// Kilobytes as megabytes for display. Resident sets are far below 2^53
/// kilobytes, so the conversion is exact.
#[allow(
    clippy::cast_precision_loss,
    reason = "resident sets are far below 2^53 kilobytes"
)]
fn megabytes(kilobytes: u64) -> f64 {
    kilobytes as f64 / 1024.0
}

/// `memory` — peak resident memory of each configured workload (spec §8.4,
/// §14). One run per workload, sampled from `/proc`; Linux only.
pub fn memory(context: &Context) -> CommandResult {
    use std::fmt::Write as _;

    let config = context.load_config()?;
    if config.workloads.is_empty() {
        return context.emit("no workloads configured; add [[workloads]] to tpt-profiler.toml");
    }
    let outcomes = match tpt_profiler::measure::collect_memory(&context.path, &config, now_unix()) {
        Ok(outcomes) => outcomes,
        Err(tpt_profiler_model::Error::ToolUnavailable(error)) => {
            return context.emit(&format!("peak memory unavailable: {error}"));
        }
        Err(error) => return Err(error.into()),
    };

    if context.format == Format::Json {
        let payload: Vec<serde_json::Value> = outcomes
            .iter()
            .map(|outcome| {
                serde_json::json!({
                    "workload": outcome.name,
                    "peak_rss_bytes": outcome.peak_rss_kb.map(|kb| kb.saturating_mul(1024)),
                    "failed": outcome.failed,
                    "measurements": outcome.measurements,
                })
            })
            .collect();
        let envelope = SchemaEnvelope::new(generated_by(), payload);
        let json = serde_json::to_string_pretty(&envelope)
            .map_err(|error| Error::Message(error.to_string()))?;
        return context.emit(&json);
    }

    let mut text = String::new();
    for outcome in &outcomes {
        let _ = match (outcome.failed, outcome.peak_rss_kb) {
            (true, _) => writeln!(text, "{}: failed or timed out; no reading", outcome.name),
            (false, None) => writeln!(
                text,
                "{}: no reading; the workload finished before the first sample",
                outcome.name
            ),
            (false, Some(kilobytes)) => writeln!(
                text,
                "{}: peak resident memory {:.1} MB (sampled, lower bound)",
                outcome.name,
                megabytes(kilobytes)
            ),
        };
    }
    let _ = write!(
        text,
        "Sampled from /proc every few milliseconds, one run per workload: Low confidence."
    );
    context.emit(&text)
}

/// `energy` — package energy used by each configured workload run (spec §56).
pub fn energy(context: &Context) -> CommandResult {
    use std::fmt::Write as _;

    let config = context.load_config()?;
    if config.workloads.is_empty() {
        return context.emit("no workloads configured; add [[workloads]] to tpt-profiler.toml");
    }
    let outcomes = match tpt_profiler::measure::collect_energy(&context.path, &config, now_unix()) {
        Ok(outcomes) => outcomes,
        Err(tpt_profiler_model::Error::ToolUnavailable(error)) => {
            return context.emit(&format!("package energy unavailable: {error}"));
        }
        Err(error) => return Err(error.into()),
    };

    if context.format == Format::Json {
        let payload: Vec<serde_json::Value> = outcomes
            .iter()
            .map(|outcome| {
                serde_json::json!({
                    "workload": outcome.name,
                    "joules": outcome.joules,
                    "failed": outcome.failed,
                    "measurements": outcome.measurements,
                })
            })
            .collect();
        let envelope = SchemaEnvelope::new(generated_by(), payload);
        let json = serde_json::to_string_pretty(&envelope)
            .map_err(|error| Error::Message(error.to_string()))?;
        return context.emit(&json);
    }

    let mut text = String::new();
    for outcome in &outcomes {
        let _ = match (outcome.failed, outcome.joules) {
            (true, _) => writeln!(text, "{}: failed or timed out; no reading", outcome.name),
            (false, None) => writeln!(text, "{}: no reading", outcome.name),
            (false, Some(joules)) => writeln!(
                text,
                "{}: {joules:.3} J (system-wide package energy, one run)",
                outcome.name
            ),
        };
    }
    let _ = write!(
        text,
        "The counter is system-wide, so other load on the machine is included: Low confidence."
    );
    context.emit(&text)
}

/// `build` — clean and incremental build times and where compile cost sits
/// (spec §8.6, §20). Runs two `cargo build`s in a tool-owned target directory.
pub fn build(context: &Context) -> CommandResult {
    use std::fmt::Write as _;

    let outcome = match tpt_profiler::build_time::measure_builds(&context.path, now_unix()) {
        Ok(outcome) => outcome,
        Err(tpt_profiler_model::Error::ToolUnavailable(error)) => {
            return context.emit(&format!("build timing unavailable: {error}"));
        }
        Err(error) => return Err(error.into()),
    };
    if context.format == Format::Json {
        let payload = serde_json::json!({
            "clean_seconds": outcome.clean_seconds,
            "incremental_seconds": outcome.incremental_seconds,
            "touched_crate": outcome.touched_crate,
            "unit_seconds": outcome.summary.unit_seconds,
            "kinds": [
                build_kind_json(&outcome.summary, tpt_profiler_build::UnitKind::Library),
                build_kind_json(&outcome.summary, tpt_profiler_build::UnitKind::BuildScript),
                build_kind_json(&outcome.summary, tpt_profiler_build::UnitKind::ProcMacro),
            ],
            "crates": outcome.summary.crates.iter().map(|crate_cost| serde_json::json!({
                "name": crate_cost.name,
                "version": crate_cost.version,
                "kind": crate_cost.kind.label(),
                "seconds": crate_cost.seconds,
                "share": crate_cost.share,
            })).collect::<Vec<_>>(),
            "findings": outcome.findings.iter().map(|finding| serde_json::json!({
                "subject": finding.subject,
                "description": finding.description,
                "confidence": finding.confidence.to_string(),
            })).collect::<Vec<_>>(),
            "measurements": outcome.measurements,
        });
        let envelope = SchemaEnvelope::new(generated_by(), payload);
        let json = serde_json::to_string_pretty(&envelope)
            .map_err(|error| Error::Message(error.to_string()))?;
        return context.emit(&json);
    }
    let summary = &outcome.summary;
    let mut text = String::new();
    let _ = writeln!(text, "Clean build:       {:.1}s", outcome.clean_seconds);
    match (outcome.incremental_seconds, &outcome.touched_crate) {
        (Some(seconds), Some(name)) => {
            let _ = writeln!(text, "Incremental build: {seconds:.1}s (touched `{name}`)");
        }
        _ => {
            let _ = writeln!(
                text,
                "Incremental build: not measured (no workspace crate to touch)"
            );
        }
    }
    let _ = writeln!(
        text,
        "
Compile-unit time: {:.1}s summed over parallel jobs",
        summary.unit_seconds
    );
    for kind in [
        tpt_profiler_build::UnitKind::Library,
        tpt_profiler_build::UnitKind::BuildScript,
        tpt_profiler_build::UnitKind::ProcMacro,
    ] {
        let _ = writeln!(
            text,
            "  {:<13} {:>8.1}s {:>5.1}%",
            kind.label(),
            summary.share_of(kind) * summary.unit_seconds,
            summary.share_of(kind) * 100.0
        );
    }
    let _ = writeln!(
        text,
        "
Top crates:"
    );
    for crate_cost in summary.crates.iter().take(10) {
        let _ = writeln!(
            text,
            "  {:<28} {:<12} {:>8.1}s {:>5.1}%  {}",
            crate_cost.name,
            crate_cost.version,
            crate_cost.seconds,
            crate_cost.share * 100.0,
            crate_cost.kind.label()
        );
    }
    if !outcome.findings.is_empty() {
        let _ = writeln!(
            text,
            "
Findings (low confidence: one timed build each):"
        );
        for finding in &outcome.findings {
            let _ = writeln!(text, "  - {}", finding.description);
        }
    }
    context.emit(text.trim_end())
}

/// `effectiveness` — decisions and outcomes per category (spec §52, §65,
/// §80). This reports the record; it does not change any heuristic.
pub fn effectiveness(context: &Context) -> CommandResult {
    use std::fmt::Write as _;

    let (_, workspace_root) = tpt_profiler::discovery::discover_repository(&context.path)?;
    let Some(tracking) = tpt_profiler::tracking::load(&workspace_root)? else {
        return context.emit("nothing recorded yet; run `baseline` first");
    };
    let rows = tracking.category_summary();
    if context.format == Format::Json {
        let json = serde_json::to_string_pretty(&SchemaEnvelope::new(generated_by(), &rows))?;
        return context.emit(&json);
    }
    let percent = |rate: Option<f64>| {
        rate.map_or_else(
            || String::from("-"),
            |value| format!("{:.0}%", value * 100.0),
        )
    };
    let mut text = String::from(
        "Decisions and outcomes by category
",
    );
    let _ = writeln!(
        text,
        "  {:<22} {:>5} {:>9} {:>8} {:>9} {:>9} {:>9} {:>10} {:>9}",
        "category",
        "seen",
        "dismissed",
        "accepted",
        "optimised",
        "validated",
        "regressed",
        "dismissal",
        "success"
    );
    for row in &rows {
        let _ = writeln!(
            text,
            "  {:<22} {:>5} {:>9} {:>8} {:>9} {:>9} {:>9} {:>10} {:>9}",
            row.category,
            row.seen,
            row.dismissed,
            row.accepted,
            row.optimised,
            row.validated,
            row.regressed,
            percent(row.dismissal_rate),
            percent(row.success_rate),
        );
    }
    context.emit(text.trim_end())
}

/// `cpu` — hardware counters and hotspots of the configured workloads under
/// Linux `perf` (spec §8.5). A missing `perf` is reported, not fatal.
pub fn cpu(context: &Context) -> CommandResult {
    use std::fmt::Write as _;

    let config = context.load_config()?;
    if config.workloads.is_empty() {
        return context.emit("no workloads configured; add [[workloads]] to tpt-profiler.toml");
    }
    let environment = tpt_profiler::environment::capture_environment(Some(&context.path));
    let outcomes = match tpt_profiler::measure::collect_counters(
        &context.path,
        &config,
        &environment,
        now_unix(),
    ) {
        Ok(outcomes) => outcomes,
        Err(tpt_profiler_model::Error::ToolUnavailable(error)) => {
            return context.emit(&format!("CPU counters unavailable: {error}"));
        }
        Err(error) => return Err(error.into()),
    };

    let sites = tpt_profiler::functions::function_sites(&context.path);
    let mut text = String::new();
    for outcome in &outcomes {
        let _ = writeln!(text, "CPU profile of `{}`", outcome.name);
        if outcome.failed {
            let _ = writeln!(text, "  failed or timed out: nothing was counted");
            continue;
        }
        let _ = writeln!(
            text,
            "  counters (mean of {} runs):",
            tpt_profiler::measure::COUNTER_REPEATS
        );
        for measurement in &outcome.measurements {
            let _ = writeln!(text, "    {}: {:.0}", measurement.metric, measurement.value);
        }
        let _ = writeln!(
            text,
            "  hotspots (leaf frames, {} Hz sampling):",
            tpt_profiler::measure::SAMPLE_HZ
        );
        if outcome.hotspots.is_empty() {
            let _ = writeln!(text, "    no samples recorded");
        }
        for hotspot in outcome.hotspots.iter().take(10) {
            let place = tpt_profiler::functions::locate(&hotspot.symbol, &sites)
                .map_or_else(String::new, |site| {
                    format!("  ({}:{})", site.file.display(), site.line)
                });
            let _ = writeln!(
                text,
                "    {:>5.1}%  {}{}",
                hotspot.percent, hotspot.symbol, place
            );
        }
    }
    context.emit(text.trim_end())
}

/// `baseline <name>` — save current measurements (spec §36).
pub fn baseline(context: &Context, name: &str) -> CommandResult {
    let config = context.load_config()?;
    let run = pipeline::analyse(&context.path, &config)?;
    let (profile, _) = measured_profile(context, &config, &run, name)?;
    let stored = tpt_profiler::baseline::baseline_from_run(name, &profile);
    let path = tpt_profiler::baseline::save_baseline(&run.workspace_root, &stored)?;
    // The reference record: later `ci` runs report opportunities that are
    // not in it as new (spec §38).
    let mut tracking = tpt_profiler::tracking::load(&run.workspace_root)?.unwrap_or_default();
    tracking.observe(&run.opportunities, now_unix());
    tpt_profiler::tracking::save(&run.workspace_root, &tracking)?;
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
    // Optimised work is validated or regressed by its own measurements
    // (spec §37, §38); `ci` only reads the record, so the change is made here.
    let mut tracking = tpt_profiler::tracking::load(&run.workspace_root)?.unwrap_or_default();
    let transitions = tracking.apply_comparison(&comparison);
    if !transitions.is_empty() {
        tpt_profiler::tracking::save(&run.workspace_root, &tracking)?;
    }
    let mut lines = vec![format!("Performance change vs `{name}`")];
    for (id, state) in &transitions {
        lines.push(format!(
            "  {id}: optimised → {}",
            tpt_profiler::tracking::state_name(state)
        ));
    }
    if let Some(warning) = &comparison.environment_warning {
        lines.push(format!("warning: {warning}"));
    }
    if comparison.metrics.is_empty() {
        lines.push(String::from(
            "no comparable measurements (run `runtime` or `bench` first)",
        ));
    }
    for metric in &comparison.metrics {
        let verdict = match metric.assessment {
            tpt_profiler::baseline::Assessment::Improved => String::from("improved"),
            tpt_profiler::baseline::Assessment::Regressed => format!(
                "REGRESSED ({} severity, {} confidence)",
                metric.severity.map_or("unknown", |grade| grade.label()),
                metric.confidence,
            ),
            tpt_profiler::baseline::Assessment::Unchanged => String::from("unchanged"),
            tpt_profiler::baseline::Assessment::Noisy => String::from("noisy — not judged"),
        };
        lines.push(format!(
            "  {}: {} {} (baseline {:.4} → current {:.4})",
            metric.metric,
            metric.change_percent(),
            verdict,
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

    // Opportunities new since the reference record, judged by the HIGH
    // priority threshold. Without a record there is nothing to compare with.
    match tpt_profiler::tracking::load(&run.workspace_root)? {
        None => outcomes.push(tpt_profiler::ci::Outcome {
            severity: tpt_profiler::ci::Severity::Info,
            message: String::from(
                "no opportunity record yet; run `baseline` on the reference branch to enable new-opportunity warnings",
            ),
        }),
        Some(tracking) => {
            let new = tracking.new_opportunities(&run.opportunities);
            outcomes.extend(tpt_profiler::ci::new_high_priority(
                &new,
                config.ci.high_priority,
            ));
        }
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
            "CI policy failed against `{name}`: a configured regression was detected"
        )));
    }
    Ok(())
}

/// Records one decision about an opportunity and saves the record. The
/// prefix is resolved against the record, which the current analysis is
/// observed into first, so an opportunity can be decided on the run that
/// first shows it (spec §38, §52).
fn record_decision(
    context: &Context,
    id: &str,
    decision: tpt_profiler::tracking::Decision,
    verb: &str,
) -> CommandResult {
    record_decision_with(context, id, verb, |_| Ok(decision))
}

/// Like [`record_decision`], but the decision is built from the record, so
/// it can refer to other opportunities by prefix (for example `supersede`).
fn record_decision_with(
    context: &Context,
    id: &str,
    verb: &str,
    build: impl FnOnce(
        &tpt_profiler::tracking::Tracking,
    ) -> Result<tpt_profiler::tracking::Decision, String>,
) -> CommandResult {
    let config = context.load_config()?;
    let run = pipeline::analyse(&context.path, &config)?;
    let now = now_unix();
    let mut tracking = tpt_profiler::tracking::load(&run.workspace_root)?.unwrap_or_default();
    tracking.observe(&run.opportunities, now);
    let target = tracking.resolve(id).map_err(Error::Message)?;
    let decision = build(&tracking).map_err(Error::Message)?;
    tracking
        .decide(&target, decision, now)
        .map_err(Error::Message)?;
    tpt_profiler::tracking::save(&run.workspace_root, &tracking)?;
    context.emit(&format!("{verb} {target}"))
}

/// `start <id>` — work on a finding has begun (spec §38).
pub fn start(context: &Context, id: &str) -> CommandResult {
    record_decision(
        context,
        id,
        tpt_profiler::tracking::Decision::Start,
        "started",
    )
}

/// `optimise <id>` — the change landed and awaits re-measurement (spec §38).
pub fn optimise(context: &Context, id: &str) -> CommandResult {
    record_decision(
        context,
        id,
        tpt_profiler::tracking::Decision::Optimise,
        "optimised",
    )
}

/// `validate <id>` — re-measurement confirmed the improvement (spec §38).
pub fn validate(context: &Context, id: &str) -> CommandResult {
    record_decision(
        context,
        id,
        tpt_profiler::tracking::Decision::Validate,
        "validated",
    )
}

/// `regress <id>` — re-measurement showed the change got worse (spec §37).
pub fn regress(context: &Context, id: &str) -> CommandResult {
    record_decision(
        context,
        id,
        tpt_profiler::tracking::Decision::Regress,
        "marked regressed",
    )
}

/// `supersede <id> --by <other>` — another finding replaces this one (spec §38).
pub fn supersede(context: &Context, id: &str, by: &str) -> CommandResult {
    record_decision_with(context, id, "superseded", |tracking| {
        let replacement = tracking.resolve(by)?;
        Ok(tpt_profiler::tracking::Decision::Supersede {
            by: tpt_profiler_model::OpportunityId::from_stored(&replacement),
        })
    })
}

/// `dismiss <id>` — mark a false positive or not-worth-doing finding.
pub fn dismiss(context: &Context, id: &str, reason: &str) -> CommandResult {
    record_decision(
        context,
        id,
        tpt_profiler::tracking::Decision::Dismiss {
            reason: reason.to_owned(),
        },
        "dismissed",
    )
}

/// `ignore <id>` — hide a finding without judging whether it is real.
pub fn ignore(context: &Context, id: &str) -> CommandResult {
    record_decision(
        context,
        id,
        tpt_profiler::tracking::Decision::Ignore,
        "ignored",
    )
}

/// `mark-not-applicable <id>` — the category does not apply to this code.
pub fn mark_not_applicable(context: &Context, id: &str, reason: &str) -> CommandResult {
    record_decision(
        context,
        id,
        tpt_profiler::tracking::Decision::NotApplicable {
            reason: reason.to_owned(),
        },
        "marked not applicable",
    )
}

/// `accept <id>` — mark a real finding that will be worked on.
pub fn accept(context: &Context, id: &str) -> CommandResult {
    record_decision(
        context,
        id,
        tpt_profiler::tracking::Decision::Accept,
        "accepted",
    )
}

/// `snooze <id> --days N` — hide a finding from reports for N days.
pub fn snooze(context: &Context, id: &str, days: u32) -> CommandResult {
    const SECONDS_PER_DAY: i64 = 86_400;
    let until = now_unix() + i64::from(days) * SECONDS_PER_DAY;
    record_decision(
        context,
        id,
        tpt_profiler::tracking::Decision::Snooze { until },
        &format!("snoozed for {days} day(s):"),
    )
}

/// `reopen <id>` — forget a dismissal, acceptance or snooze.
pub fn reopen(context: &Context, id: &str) -> CommandResult {
    record_decision(
        context,
        id,
        tpt_profiler::tracking::Decision::Reopen,
        "reopened",
    )
}

/// `report <name>` — print results (spec §42). `latest` re-runs the
/// analysis in-process; any other name prints a run saved by
/// `full --save <name>` without re-analysing the source.
pub fn report(context: &Context, name: &str) -> CommandResult {
    if name == "latest" {
        let config = context.load_config()?;
        let mut run = pipeline::analyse(&context.path, &config)?;
        apply_tracking(&run.workspace_root, &mut run.opportunities)?;
        return emit_analysis(context, &run);
    }
    let (repository, workspace_root) = tpt_profiler::discovery::discover_repository(&context.path)?;
    let Some(stored) = tpt_profiler::runs::load_run(&workspace_root, name)? else {
        return Err(Error::Message(format!(
            "stored run `{name}` not found — save one with `tpt-profiler full --save {name}`"
        )));
    };
    let mut opportunities = stored.opportunities;
    apply_tracking(&workspace_root, &mut opportunities)?;
    let repository_name = opportunities
        .first()
        .map_or(repository.name.as_str(), |opportunity| {
            opportunity.repository.as_str()
        });
    write_summary(context, repository_name, &opportunities)?;
    emit_opportunities(context, repository_name, &opportunities)
}

/// Applies the recorded decisions to an analysis: each opportunity shows
/// its lifecycle state, and those dismissed or still snoozed are hidden
/// (spec §38). With no record yet, the opportunities are left as they are.
fn apply_tracking(
    workspace_root: &std::path::Path,
    opportunities: &mut Vec<tpt_profiler_model::Opportunity>,
) -> Result<(), Error> {
    if let Some(tracking) = tpt_profiler::tracking::load(workspace_root)? {
        let now = now_unix();
        tracking.apply(opportunities);
        opportunities.retain(|opportunity| !tracking.is_suppressed(opportunity.id.as_str(), now));
    }
    Ok(())
}

/// `opportunities --strategy --top` (spec §41, §59).
pub fn opportunities(
    context: &Context,
    strategy: tpt_profiler_opportunity::Strategy,
    top: usize,
) -> CommandResult {
    let config = context.load_config()?;
    let mut run = pipeline::analyse(&context.path, &config)?;
    apply_tracking(&run.workspace_root, &mut run.opportunities)?;
    let mut opportunities = run.opportunities.clone();
    tpt_profiler_opportunity::sort_by_strategy(&mut opportunities, strategy);
    opportunities.truncate(top);
    write_summary(context, &run.repository.name, &opportunities)?;
    if run.no_opportunities && context.format == Format::Terminal {
        return context.emit("No opportunities detected.");
    }
    emit_opportunities(context, &run.repository.name, &opportunities)
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
