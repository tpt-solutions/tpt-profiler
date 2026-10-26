//! Discovery integration tests against fixture workspaces (spec §7, §76):
//! single crate, multi-crate workspace, and virtual workspace.

use std::path::{Path, PathBuf};
use tpt_profiler::discovery::{discover_repository, source_files};
use tpt_profiler_model::hierarchy::DependencyKind;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

#[test]
fn discovers_single_crate_project() {
    let (repository, root) = discover_repository(&fixture("single-crate")).expect("discovery");
    assert_eq!(repository.projects.len(), 1);
    assert_eq!(repository.name, "single-crate");
    assert_eq!(root.file_name().unwrap().to_string_lossy(), "single-crate");

    let project = &repository.projects[0];
    assert_eq!(project.crates.len(), 1);
    let info = &project.crates[0];
    assert_eq!(info.name, "tpt-fixture-single-crate");
    assert!(info.has_tests, "tests/smoke.rs must be discovered");
    assert!(info.has_benches, "benches/basic.rs must be discovered");
    assert!(!info.uses_criterion);
    assert_eq!(info.dependencies.len(), 0);
}

#[test]
fn discovers_multi_crate_workspace_with_internal_deps() {
    let (repository, _root) = discover_repository(&fixture("multi-crate")).expect("discovery");
    let project = &repository.projects[0];
    let names: Vec<_> = project.crates.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["tpt-fixture-member-a", "tpt-fixture-member-b"],
        "members must be sorted (spec §75)"
    );

    let b = &project.crates[1];
    assert_eq!(b.dependencies.len(), 1);
    let dep = &b.dependencies[0];
    assert_eq!(dep.name, "tpt-fixture-member-a");
    assert_eq!(dep.kind, DependencyKind::Normal);
    assert!(dep.internal, "path dep inside the workspace is internal");
}

#[test]
fn discovers_virtual_workspace_without_root_package() {
    let (repository, _root) =
        discover_repository(&fixture("virtual-workspace")).expect("discovery");
    let project = &repository.projects[0];
    let names: Vec<_> = project.crates.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["tpt-fixture-alpha", "tpt-fixture-beta"],
        "virtual workspaces list all members"
    );
    let beta = &project.crates[1];
    assert!(beta.dependencies.iter().any(|d| d.internal));
}

#[test]
fn discovery_fails_cleanly_outside_a_cargo_project() {
    let temp = std::env::temp_dir().join("tpt-discovery-no-manifest");
    std::fs::create_dir_all(&temp).expect("mkdir");
    let error = discover_repository(&temp).expect_err("no Cargo.toml here");
    let rendered = format!("{error}");
    assert!(
        rendered.contains("cargo metadata") || rendered.contains("Cargo.toml"),
        "unexpected error: {rendered}"
    );
    std::fs::remove_dir_all(&temp).ok();
}

#[test]
fn source_walk_finds_fixture_sources_without_gitignored_files() {
    let root = fixture("single-crate");
    let mut files: Vec<_> = source_files(&root).collect();
    files.sort();
    let names: Vec<String> = files
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert!(
        names.contains(&String::from("lib.rs")),
        "lib.rs must be walked: {names:?}"
    );
    assert!(
        !names.iter().any(|name| name.starts_with("ignored")),
        "gitignored sources must be skipped: {names:?}"
    );
}
