//! `tpt-profiler` — command-line front end (spec §41).
//!
//! Exit codes (spec §74: "no opportunity detected" is a result, not a
//! failure):
//!
//! * `0` — the command ran successfully (including empty results);
//! * `1` — the command failed (invalid config, missing tool, corrupt
//!   baseline, spawn failure, …).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};

mod commands;

/// tpt-profiler: which parts of this project are worth optimising next,
/// why, by how much, and with what confidence (spec §1).
#[derive(Debug, Parser)]
#[command(name = "tpt-profiler", version, about, long_about)]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,

    /// Output format for report-producing commands (spec §42).
    #[arg(long, value_enum, default_value_t = Format::Terminal, global = true)]
    format: Format,

    /// Write the report to a file instead of stdout (spec §42).
    #[arg(long, value_name = "FILE", global = true)]
    output: Option<PathBuf>,

    /// Append a Markdown summary of the results to FILE, for CI job
    /// summaries (e.g. `$GITHUB_STEP_SUMMARY`). Opt-in and separate from
    /// `--format`, so stdout output is unchanged (spec §39).
    #[arg(long, value_name = "FILE", global = true)]
    summary: Option<PathBuf>,

    /// Path to `tpt-profiler.toml` (default: discovered upward).
    #[arg(long, value_name = "FILE", global = true)]
    config: Option<PathBuf>,

    /// Project path to analyse (default: the current directory).
    #[arg(long, value_name = "PATH", default_value = ".", global = true)]
    path: PathBuf,

    /// Suppress informational output.
    #[arg(long, global = true)]
    quiet: bool,

    /// Verbose diagnostics on stderr.
    #[arg(long, global = true)]
    verbose: bool,

    /// Turn off profiling or benchmark modes for this run, overriding the
    /// config file (spec §40: flags > config > defaults). Comma-separated;
    /// this only disables, so enable modes in `tpt-profiler.toml`.
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        value_name = "MODE",
        global = true
    )]
    disable: Vec<Mode>,
}

/// Report-producing output formats (spec §42).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Format {
    /// Human-readable terminal output (honours NO_COLOR by disabling
    /// colour; see `commands::should_colour`).
    Terminal,
    /// Versioned JSON (spec §69).
    Json,
    /// JSON Lines: one opportunity per line (spec §42).
    Jsonl,
    /// Markdown.
    Markdown,
    /// Self-contained HTML page (spec §42).
    Html,
    /// GitHub Actions workflow annotations (spec §39).
    GithubAnnotations,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    /// Discover and print the project structure (spec §7).
    Scan,
    /// Run static analysis and report opportunities (spec §8.1).
    Static,
    /// Run configured workloads and report wall-time measurements
    /// (spec §8.2).
    Runtime,
    /// Execute benchmarks and ingest Criterion results (spec §8.3).
    Bench,
    /// Decisions and outcomes per category: dismissal and success rates
    /// (spec §52, §65, §80).
    Effectiveness,
    /// Hardware counters and hotspots of the workloads under Linux `perf`
    /// (spec §8.5).
    Cpu,
    /// Scan many repositories under a directory and report what they share
    /// (spec §33, §34, §60). Static and local only.
    Ecosystem {
        /// Directory to search for repositories (default: `--path`).
        root: Option<PathBuf>,
        /// How many directory levels below the root to search.
        #[arg(long, default_value_t = tpt_profiler::ecosystem::DEFAULT_MAX_DEPTH)]
        max_depth: usize,
        /// Score bonus per extra repository sharing an opportunity.
        #[arg(long, default_value_t = tpt_profiler::ecosystem::DEFAULT_SHARED_WEIGHT)]
        shared_weight: f64,
        /// How many rows to show in each table.
        #[arg(long, default_value_t = 10)]
        top: usize,
        /// Repositories analysed at once.
        #[arg(long, default_value_t = tpt_profiler::ecosystem::default_workers())]
        workers: usize,
    },
    /// Run every applicable analysis (spec §8.7).
    Full {
        /// Also store the run under this name, for `report <name>`.
        #[arg(long, value_name = "NAME")]
        save: Option<String>,
    },
    /// Save the current measurements as a named baseline (spec §36).
    Baseline {
        /// Baseline name (default: `default`).
        #[arg(default_value = "default")]
        name: String,
    },
    /// Compare the current measurements against a baseline (spec §36).
    Compare {
        /// Baseline name to compare against (default: `default`).
        #[arg(default_value = "default")]
        name: String,
    },
    /// Apply the `[ci]` policy to the current measurements against a
    /// baseline; fails the run on a configured regression (spec §39).
    Ci {
        /// Baseline name to compare against (default: `default`).
        #[arg(default_value = "default")]
        name: String,
    },
    /// Record that a finding is a false positive or not worth doing
    /// (spec §38). The reason is kept in the record.
    Dismiss {
        /// Opportunity ID or a unique prefix.
        id: String,
        /// Why it is dismissed.
        #[arg(long, default_value = "dismissed by user")]
        reason: String,
    },
    /// Hide a finding from reports without judging whether it is real
    /// (spec §52). Reopen it to show it again.
    Ignore {
        /// Opportunity ID or a unique prefix.
        id: String,
    },
    /// Record that a finding's category does not apply to this code
    /// (spec §52). The reason is kept in the record.
    MarkNotApplicable {
        /// Opportunity ID or a unique prefix.
        id: String,
        /// Why the category does not apply.
        #[arg(long)]
        reason: String,
    },
    /// Record that work on a finding has started (spec §38).
    Start {
        /// Opportunity ID or a unique prefix.
        id: String,
    },
    /// Record that the change for a finding landed and awaits re-measurement
    /// (spec §38).
    Optimise {
        /// Opportunity ID or a unique prefix.
        id: String,
    },
    /// Record that re-measurement confirmed the improvement (spec §38).
    Validate {
        /// Opportunity ID or a unique prefix.
        id: String,
    },
    /// Record that re-measurement showed the change got worse (spec §37).
    Regress {
        /// Opportunity ID or a unique prefix.
        id: String,
    },
    /// Record that another finding replaces this one (spec §38).
    Supersede {
        /// Opportunity ID or a unique prefix of the finding being replaced.
        id: String,
        /// Opportunity ID or a unique prefix of the replacement.
        #[arg(long)]
        by: String,
    },
    /// Record that a finding is real and will be worked on (spec §38).
    Accept {
        /// Opportunity ID or a unique prefix.
        id: String,
    },
    /// Hide a finding from reports for a number of days (spec §38).
    Snooze {
        /// Opportunity ID or a unique prefix.
        id: String,
        /// How many days to hide it.
        #[arg(long, default_value_t = 30)]
        days: u32,
    },
    /// Forget a dismissal, acceptance or snooze (spec §38).
    Reopen {
        /// Opportunity ID or a unique prefix.
        id: String,
    },
    /// Print stored run results (spec §42).
    Report {
        /// Name of a stored run to print (default: latest analysis).
        #[arg(default_value = "latest")]
        name: String,
    },
    /// Rank optimisation opportunities (spec §41, §59).
    Opportunities {
        /// Prioritisation strategy.
        #[arg(long, value_enum, default_value_t = StrategyArg::HighestValue)]
        strategy: StrategyArg,
        /// How many opportunities to show.
        #[arg(long, default_value_t = 10)]
        top: usize,
    },
    /// Explain one opportunity in full (spec §41).
    Explain {
        /// The opportunity ID (e.g. SIMD-04217); a prefix is accepted.
        id: String,
    },
    /// Generate shell completions.
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Print the man page (roff) to stdout, e.g. `tpt-profiler man > tpt-profiler.1`.
    Man,
}

/// `--strategy` values (spec §59).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum StrategyArg {
    /// Highest expected value first (priority × confidence × generality
    /// over effort × risk).
    HighestValue,
}

impl From<StrategyArg> for tpt_profiler_opportunity::Strategy {
    fn from(strategy: StrategyArg) -> Self {
        match strategy {
            StrategyArg::HighestValue => Self::HighestValue,
        }
    }
}

/// Modes that `--disable` can turn off (spec §40).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Mode {
    Runtime,
    Memory,
    Cpu,
    Simd,
    Parallelism,
    Benchmarks,
    Ci,
}

/// Maps `--disable` modes onto config overrides (flags > config).
fn overrides_from(disabled: &[Mode]) -> tpt_profiler::config::Overrides {
    let mut overrides = tpt_profiler::config::Overrides::default();
    for mode in disabled {
        match mode {
            Mode::Runtime => overrides.profiling_runtime = Some(false),
            Mode::Memory => overrides.profiling_memory = Some(false),
            Mode::Cpu => overrides.profiling_cpu = Some(false),
            Mode::Simd => overrides.profiling_simd = Some(false),
            Mode::Parallelism => overrides.profiling_parallelism = Some(false),
            Mode::Benchmarks => overrides.benchmarks_enabled = Some(false),
            Mode::Ci => overrides.ci_enabled = Some(false),
        }
    }
    overrides
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let context = commands::Context {
        format: match cli.format {
            Format::Terminal => commands::Format::Terminal,
            Format::Json => commands::Format::Json,
            Format::Jsonl => commands::Format::Jsonl,
            Format::Markdown => commands::Format::Markdown,
            Format::Html => commands::Format::Html,
            Format::GithubAnnotations => commands::Format::GithubAnnotations,
        },
        output: cli.output,
        summary: cli.summary,
        config_path: cli.config,
        path: cli.path,
        quiet: cli.quiet,
        verbose: cli.verbose,
        overrides: overrides_from(&cli.disable),
    };
    let result = match cli.command {
        CliCommand::Scan => commands::scan(&context),
        CliCommand::Static => commands::static_analysis(&context),
        CliCommand::Runtime => commands::runtime(&context),
        CliCommand::Bench => commands::bench(&context),
        CliCommand::Effectiveness => commands::effectiveness(&context),
        CliCommand::Cpu => commands::cpu(&context),
        CliCommand::Ecosystem {
            root,
            max_depth,
            shared_weight,
            top,
            workers,
        } => {
            let root = root.unwrap_or_else(|| context.path.clone());
            let options = tpt_profiler::ecosystem::EcosystemOptions {
                max_depth,
                shared_weight,
                workers,
            };
            commands::ecosystem(&context, &root, &options, top)
        }
        CliCommand::Full { save } => commands::full(&context, save.as_deref()),
        CliCommand::Baseline { name } => commands::baseline(&context, &name),
        CliCommand::Compare { name } => commands::compare(&context, &name),
        CliCommand::Ci { name } => commands::ci(&context, &name),
        CliCommand::Dismiss { id, reason } => commands::dismiss(&context, &id, &reason),
        CliCommand::Ignore { id } => commands::ignore(&context, &id),
        CliCommand::MarkNotApplicable { id, reason } => {
            commands::mark_not_applicable(&context, &id, &reason)
        }
        CliCommand::Start { id } => commands::start(&context, &id),
        CliCommand::Optimise { id } => commands::optimise(&context, &id),
        CliCommand::Validate { id } => commands::validate(&context, &id),
        CliCommand::Regress { id } => commands::regress(&context, &id),
        CliCommand::Supersede { id, by } => commands::supersede(&context, &id, &by),
        CliCommand::Accept { id } => commands::accept(&context, &id),
        CliCommand::Snooze { id, days } => commands::snooze(&context, &id, days),
        CliCommand::Reopen { id } => commands::reopen(&context, &id),
        CliCommand::Report { name } => commands::report(&context, &name),
        CliCommand::Opportunities { strategy, top } => {
            commands::opportunities(&context, strategy.into(), top)
        }
        CliCommand::Explain { id } => commands::explain(&context, &id),
        CliCommand::Completions { shell } => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "tpt-profiler", &mut std::io::stdout());
            Ok(())
        }
        CliCommand::Man => clap_mangen::Man::new(Cli::command())
            .render(&mut std::io::stdout())
            .map_err(|error| {
                commands::Error::Message(format!("could not render man page: {error}"))
            }),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
