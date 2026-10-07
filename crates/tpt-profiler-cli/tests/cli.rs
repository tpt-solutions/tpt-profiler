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
