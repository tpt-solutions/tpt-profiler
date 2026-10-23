//! `tpt-profiler` — command-line front end (spec §41).
//!
//! Exit codes (spec §74: "no opportunity detected" is a result, not a
//! failure):
//!
//! * `0` — the command ran successfully (including empty results);
//! * `1` — the command failed (invalid config, missing tool, corrupt
//!   baseline, spawn failure, …).

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
    output: Option<std::path::PathBuf>,

    /// Append a Markdown summary of the results to FILE, for CI job
    /// summaries (e.g. `$GITHUB_STEP_SUMMARY`). Opt-in and separate from
    /// `--format`, so stdout output is unchanged (spec §39).
    #[arg(long, value_name = "FILE", global = true)]
    summary: Option<std::path::PathBuf>,

    /// Path to `tpt-profiler.toml` (default: discovered upward).
    #[arg(long, value_name = "FILE", global = true)]
    config: Option<std::path::PathBuf>,

    /// Project path to analyse (default: the current directory).
    #[arg(long, value_name = "PATH", default_value = ".", global = true)]
    path: std::path::PathBuf,

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
    /// Run every applicable analysis (spec §8.7).
    Full,
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
        CliCommand::Full => commands::full(&context),
        CliCommand::Baseline { name } => commands::baseline(&context, &name),
        CliCommand::Compare { name } => commands::compare(&context, &name),
        CliCommand::Ci { name } => commands::ci(&context, &name),
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
