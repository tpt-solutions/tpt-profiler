//! End-to-end benchmark execution (spec §8.3, §76): `run_benches` must
//! really run `cargo bench` and ingest what the bench target wrote.
//!
//! The fixture is a throwaway crate whose bench target (`harness = false`)
//! writes Criterion-shaped `estimates.json`, so the test exercises the
//! real process runner and the real result parser without depending on
//! Criterion itself being installed.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tpt_profiler_bench::run_benches;
use tpt_profiler_model::hierarchy::CrateInfo;

/// Removes the fixture directory when the test ends, even on panic.
struct TempWorkspace(PathBuf);

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("has parent")).expect("create dirs");
    fs::write(path, contents).expect("write fixture file");
}

fn create_fixture(name: &str) -> TempWorkspace {
    let root = std::env::temp_dir().join(format!("tpt-bench-exec-{name}-{}", std::process::id()));
    // A stale directory from an interrupted run would mask a failure.
    let _ = fs::remove_dir_all(&root);
    write(
        &root.join("Cargo.toml"),
        r#"[package]
name = "bench-fixture"
version = "0.1.0"
edition = "2021"

[[bench]]
name = "fake"
harness = false
"#,
    );
    write(&root.join("src").join("lib.rs"), "");
    write(
        &root.join("benches").join("fake.rs"),
        r##"// Mimics Criterion's output layout for one benchmark named `fake_op`.
// Mean 1500 ns, so the profiler should report 1.5e-6 s.
fn main() {
    let dir = std::path::Path::new("target/criterion/fake_op/new");
    std::fs::create_dir_all(dir).expect("create criterion dir");
    let json = r#"{
        "mean": {
            "confidence_interval": { "confidence_level": 0.95, "lower_bound": 1400.0, "upper_bound": 1600.0 },
            "point_estimate": 1500.0,
            "standard_deviation": 50.0
        }
    }"#;
    std::fs::write(dir.join("estimates.json"), json).expect("write estimates");
}
"##,
    );
    TempWorkspace(root)
}

fn fixture_crate() -> CrateInfo {
    CrateInfo {
        name: String::from("bench-fixture"),
        version: String::from("0.1.0"),
        has_benches: true,
        ..CrateInfo::default()
    }
}

#[test]
fn cargo_bench_runs_and_criterion_results_are_ingested() {
    let workspace = create_fixture("ingest");
    let report = run_benches(&workspace.0, &[fixture_crate()], Duration::from_secs(300))
        .expect("cargo bench runs");

    assert!(!report.no_benchmarks, "bench target was discovered");
    let estimate = report
        .estimates
        .get("fake_op")
        .unwrap_or_else(|| panic!("fake_op ingested; output:\n{}", report.cargo_output));
    assert!((estimate.point_estimate - 1500.0).abs() < f64::EPSILON);
}

#[test]
fn bench_target_failure_is_reported_as_an_error() {
    let workspace = create_fixture("failure");
    fs::write(
        workspace.0.join("benches").join("fake.rs"),
        "fn main() { std::process::exit(3); }\n",
    )
    .expect("overwrite bench");

    let error = run_benches(&workspace.0, &[fixture_crate()], Duration::from_secs(300))
        .expect_err("a failing bench is a benchmark error, not an empty result");
    assert!(error.to_string().contains("cargo bench"), "{error}");
}
