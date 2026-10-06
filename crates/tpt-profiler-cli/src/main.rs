//! `tpt-profiler` — command-line front end (spec §41).
//!
//! The CLI wires configuration, the core orchestrator, and report
//! rendering together. Subcommands (`scan`, `static`, `runtime`, `bench`,
//! `full`, `baseline`, `compare`, `report`, `opportunities`, `explain`)
//! land with the MVP; see the repository README for status.

fn main() {
    println!("tpt-profiler {}", tpt_profiler::model::version());
    eprintln!("under construction: see spec.txt §41 for the planned command set");
}
