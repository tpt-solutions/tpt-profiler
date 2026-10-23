//! Runs the static-side pipeline over a project and prints the ranked
//! opportunities.
//!
//! ```text
//! cargo run -p tpt-profiler --example analyse -- path/to/project
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use tpt_profiler::config::Config;
use tpt_profiler::pipeline;

fn main() -> ExitCode {
    let root = std::env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);

    match pipeline::analyse(&root, &Config::default()) {
        Ok(run) if run.no_opportunities => {
            println!("{}: no opportunities detected", run.repository.name);
            ExitCode::SUCCESS
        }
        Ok(run) => {
            for opportunity in &run.opportunities {
                println!(
                    "{}\t{}\t{}",
                    opportunity.id, opportunity.component, opportunity.description
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
