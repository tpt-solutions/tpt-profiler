//! CLI integration tests (spec §76): the binary must run the real
//! pipeline over fixtures with meaningful exit codes (spec §74: "no
//! opportunity detected" exits 0).

use assert_cmd::Command;

fn binary() -> Command {
    Command::cargo_bin("tpt-profiler").expect("binary builds")
}

fn fixture() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/single-crate"
    )
}

#[test]
fn version_flag_prints_version() {
    let output = binary().arg("--version").output().expect("run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(stdout.contains("tpt-profiler"), "{stdout}");
}

#[test]
fn scan_prints_the_fixture_tree() {
    let output = binary()
        .args(["--path", fixture(), "scan"])
        .output()
        .expect("run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(stdout.contains("Repository: single-crate"), "{stdout}");
    assert!(stdout.contains("tpt-fixture-single-crate"), "{stdout}");
    assert!(stdout.contains("bench, test"), "{stdout}");
}

#[test]
fn static_run_on_clean_fixture_exits_zero_with_stated_result() {
    let output = binary()
        .args(["--path", fixture(), "--quiet", "static"])
        .output()
        .expect("run");
    assert!(
        output.status.success(),
        "no opportunities is success (spec §74): {:?}",
        output.stderr
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(stdout.contains("No opportunities detected."), "{stdout}");
}

#[test]
fn help_lists_the_spec_41_commands() {
    let output = binary().arg("--help").output().expect("run");
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    for command in [
        "scan",
        "static",
        "runtime",
        "bench",
        "full",
        "baseline",
        "compare",
        "report",
        "opportunities",
        "explain",
        "completions",
    ] {
        assert!(stdout.contains(command), "missing `{command}` in help");
    }
}

#[test]
fn explain_with_unknown_id_fails_with_message() {
    let output = binary()
        .args(["--path", fixture(), "--quiet", "explain", "NOPE-00001"])
        .output()
        .expect("run");
    assert!(!output.status.success(), "unknown id is a failure");
    let stderr = String::from_utf8(output.stderr).expect("utf8");
    assert!(stderr.contains("NOPE-00001"), "{stderr}");
}

#[test]
fn compare_without_baseline_fails_with_guidance() {
    let output = binary()
        .args(["--path", fixture(), "--quiet", "compare", "nosuch"])
        .output()
        .expect("run");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8");
    assert!(stderr.contains("baseline `nosuch` not found"), "{stderr}");
}

#[test]
fn completions_generate_for_bash() {
    let output = binary()
        .args(["completions", "bash"])
        .output()
        .expect("run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(stdout.contains("tpt-profiler"), "{stdout}");
}

/// A temporary project whose single source file is the given corpus
/// pattern, so the pipeline has known-detectable findings to report.
struct CorpusProject(std::path::PathBuf);

impl CorpusProject {
    fn new(name: &str, corpus_file: &str) -> Self {
        let root = std::env::temp_dir().join(format!("tpt-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).expect("create src");
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"corpus-project\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("write manifest");
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tpt-profiler-static/tests/corpus")
            .join(corpus_file);
        std::fs::copy(source, root.join("src").join("lib.rs")).expect("copy corpus");
        Self(root)
    }

    fn path(&self) -> &str {
        self.0.to_str().expect("utf8 temp path")
    }
}

impl Drop for CorpusProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn man_page_is_roff_for_the_binary() {
    let output = binary().arg("man").output().expect("run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(stdout.contains(".TH tpt-profiler 1"), "{stdout}");
}

#[test]
fn json_opportunities_are_wrapped_in_the_schema_envelope() {
    let project = CorpusProject::new("json", "hot_loop.rs");
    let output = binary()
        .args([
            "--path",
            project.path(),
            "--quiet",
            "--format",
            "json",
            "opportunities",
        ])
        .output()
        .expect("run");
    assert!(output.status.success());
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout is JSON, not prose");
    assert_eq!(value["schema_version"], 1);
    assert!(
        value["generated_by"]
            .as_str()
            .is_some_and(|s| s.starts_with("tpt-profiler "))
    );
    let entries = value["data"]["opportunities"].as_array().expect("array");
    assert!(!entries.is_empty(), "hot_loop corpus has findings");
    assert!(entries[0]["id"].as_str().is_some_and(|id| id.contains('-')));
}

#[test]
fn json_with_no_opportunities_is_an_empty_list_not_prose() {
    let output = binary()
        .args([
            "--path",
            fixture(),
            "--quiet",
            "--format",
            "json",
            "opportunities",
        ])
        .output()
        .expect("run");
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(value["data"]["opportunities"], serde_json::json!([]));
}

#[test]
fn output_flag_writes_the_report_to_a_file() {
    let project = CorpusProject::new("output", "clean.rs");
    let target = project.0.join("report.json");
    let output = binary()
        .args([
            "--path",
            project.path(),
            "--quiet",
            "--format",
            "json",
            "--output",
            target.to_str().expect("utf8"),
            "static",
        ])
        .output()
        .expect("run");
    assert!(output.status.success());
    assert!(
        output.stdout.is_empty(),
        "nothing on stdout when --output is set"
    );
    let written = std::fs::read_to_string(&target).expect("report written");
    assert!(written.contains("\"schema_version\""), "{written}");
}

#[test]
fn disable_simd_removes_simd_opportunities() {
    let project = CorpusProject::new("disable-simd", "hot_loop.rs");
    let categories = |args: &[&str]| -> Vec<String> {
        let output = binary()
            .args(["--path", project.path(), "--quiet", "--format", "json"])
            .args(args)
            .output()
            .expect("run");
        assert!(output.status.success());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
        value["data"]["opportunities"]
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|entry| entry["category"].as_str().map(str::to_owned))
            .collect()
    };
    assert!(categories(&["opportunities"]).iter().any(|c| c == "SIMD"));
    assert!(
        !categories(&["--disable", "simd", "opportunities"])
            .iter()
            .any(|c| c == "SIMD")
    );
}

#[test]
fn disabled_runtime_says_so_instead_of_running_workloads() {
    let output = binary()
        .args([
            "--path",
            fixture(),
            "--quiet",
            "--disable",
            "runtime",
            "runtime",
        ])
        .output()
        .expect("run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(stdout.contains("runtime profiling is disabled"), "{stdout}");
}

#[test]
fn disabled_benchmarks_are_not_run() {
    let output = binary()
        .args([
            "--path",
            fixture(),
            "--quiet",
            "--disable",
            "benchmarks",
            "bench",
        ])
        .output()
        .expect("run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(stdout.contains("benchmarks are disabled"), "{stdout}");
}

#[test]
fn summary_flag_appends_markdown_and_leaves_stdout_alone() {
    let directory = std::env::temp_dir().join(format!("tpt-summary-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("create temp dir");
    let summary = directory.join("summary.md");
    let _ = std::fs::remove_file(&summary);

    for _ in 0..2 {
        binary()
            .args(["--path", fixture(), "--quiet", "--summary"])
            .arg(&summary)
            .args(["--format", "jsonl", "static"])
            .assert()
            .success();
    }

    let written = std::fs::read_to_string(&summary).expect("summary written");
    assert_eq!(
        written.matches("# tpt-profiler report").count(),
        2,
        "append mode keeps both runs: {written}"
    );
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn dismissed_opportunity_is_hidden_from_later_reports() {
    let root = std::env::temp_dir().join(format!("tpt-dismiss-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("create project");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"dismiss-probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write manifest");
    std::fs::write(
        root.join("src").join("lib.rs"),
        "pub fn names(values: &[String]) -> Vec<String> {\n    let mut out = Vec::new();\n    for value in values {\n        out.push(value.clone());\n    }\n    out\n}\n",
    )
    .expect("write source");

    let listing = |root: &std::path::Path| {
        let output = binary()
            .arg("--path")
            .arg(root)
            .args(["--quiet", "--format", "jsonl", "opportunities"])
            .output()
            .expect("run");
        assert!(output.status.success());
        String::from_utf8(output.stdout).expect("utf8")
    };

    let before = listing(&root);
    let id = before
        .split("\"id\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("an opportunity to dismiss")
        .to_owned();

    binary()
        .arg("--path")
        .arg(&root)
        .args(["dismiss", &id, "--reason", "test"])
        .assert()
        .success();

    let after = listing(&root);
    let report = binary()
        .arg("--path")
        .arg(&root)
        .args(["--quiet", "--format", "jsonl", "report"])
        .output()
        .expect("run");
    let report = String::from_utf8(report.stdout).expect("utf8");
    std::fs::remove_dir_all(&root).ok();
    assert!(
        !after.contains(&id),
        "dismissed {id} still listed:\n{after}"
    );
    assert!(
        !report.contains(&id),
        "dismissed {id} still in report:\n{report}"
    );
}

#[test]
fn saved_run_is_reported_without_reanalysing_and_unknown_names_fail() {
    let project = CorpusProject::new("stored-run", "hot_loop.rs");

    binary()
        .args([
            "--path",
            project.path(),
            "--quiet",
            "full",
            "--save",
            "nightly",
        ])
        .assert()
        .success();
    let stored = std::path::Path::new(project.path()).join(".tpt-profiler/runs/nightly.json");
    assert!(
        stored.exists(),
        "run was not stored at {}",
        stored.display()
    );

    let fresh = binary()
        .args([
            "--path",
            project.path(),
            "--quiet",
            "--format",
            "jsonl",
            "opportunities",
        ])
        .output()
        .expect("run");
    let stored_report = binary()
        .args([
            "--path",
            project.path(),
            "--quiet",
            "--format",
            "jsonl",
            "report",
            "nightly",
        ])
        .output()
        .expect("run");
    assert!(stored_report.status.success());
    let fresh_ids: Vec<String> = String::from_utf8_lossy(&fresh.stdout)
        .lines()
        .filter_map(|line| {
            line.split("\"id\":\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
        })
        .map(str::to_owned)
        .collect();
    let stored_ids: Vec<String> = String::from_utf8_lossy(&stored_report.stdout)
        .lines()
        .filter_map(|line| {
            line.split("\"id\":\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
        })
        .map(str::to_owned)
        .collect();
    assert!(
        !stored_ids.is_empty(),
        "hot loop should yield opportunities"
    );
    assert_eq!(stored_ids, fresh_ids);

    let missing = binary()
        .args(["--path", project.path(), "--quiet", "report", "nosuch"])
        .output()
        .expect("run");
    assert!(!missing.status.success());
    let stderr = String::from_utf8(missing.stderr).expect("utf8");
    assert!(stderr.contains("full --save nosuch"), "{stderr}");
}

#[test]
fn cpu_without_workloads_says_what_to_configure() {
    let project = CorpusProject::new("cpu-empty", "clean.rs");
    let output = binary()
        .args(["--path", project.path(), "--quiet", "cpu"])
        .output()
        .expect("run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    assert!(stdout.contains("no workloads configured"), "{stdout}");
}

#[test]
fn ecosystem_reports_findings_shared_across_repositories() {
    let root = std::env::temp_dir().join(format!("tpt-cli-eco-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tpt-profiler-static/tests/corpus/hot_loop.rs");
    for name in ["alpha", "beta"] {
        let repo = root.join(name);
        std::fs::create_dir_all(repo.join("src")).expect("repo");
        std::fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = \"eco\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("manifest");
        std::fs::copy(&corpus, repo.join("src").join("lib.rs")).expect("corpus");
    }

    let text = binary()
        .args([
            "--path",
            root.to_str().expect("utf8"),
            "--quiet",
            "ecosystem",
            "--top",
            "5",
        ])
        .output()
        .expect("run");
    assert!(text.status.success());
    let stdout = String::from_utf8(text.stdout).expect("utf8");
    assert!(stdout.contains("2 repositories (0 failed)"), "{stdout}");
    assert!(stdout.contains("Shared opportunities"), "{stdout}");
    assert!(stdout.contains("2 repos"), "{stdout}");

    let json = binary()
        .args([
            "--path",
            root.to_str().expect("utf8"),
            "--quiet",
            "--format",
            "json",
            "ecosystem",
        ])
        .output()
        .expect("run");
    let json = String::from_utf8(json.stdout).expect("utf8");
    std::fs::remove_dir_all(&root).ok();
    assert!(json.contains("\"schema_version\""), "{json}");
    assert!(json.contains("\"shared\""), "{json}");
}
