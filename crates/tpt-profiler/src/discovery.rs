//! Repository and workspace discovery (spec §7).
//!
//! Wraps `cargo metadata` as the evidence provider for project structure
//! (spec §68): workspace members, targets, features, and dependencies are
//! mapped onto the model hierarchy in [`tpt_profiler_model::hierarchy`].
//! Source walking is gitignore-aware via the `ignore` crate.

use std::path::{Path, PathBuf};

use cargo_metadata::{DependencyKind as CargoDepKind, Package, TargetKind};
use tpt_profiler_model::error::{DiscoveryError, Error};
use tpt_profiler_model::hierarchy::{
    CrateInfo, DependencyInfo, DependencyKind, Project, Repository,
};

/// Discovers the Cargo workspace at or above `start` and maps it onto a
/// [`Repository`].
///
/// Returns the repository plus the workspace root that was analysed.
/// Single-crate projects are modelled as a one-member workspace; virtual
/// workspaces (no root package) list their members.
pub fn discover_repository(start: &Path) -> Result<(Repository, PathBuf), Error> {
    let manifest = if start.is_file() {
        start.to_path_buf()
    } else {
        start.join("Cargo.toml")
    };
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(&manifest)
        .exec()
        .map_err(|error| {
            Error::Discovery(DiscoveryError::MetadataFailed {
                message: error.to_string(),
            })
        })?;

    let workspace_root = metadata.workspace_root.clone();
    let member_ids: std::collections::HashSet<_> = metadata.workspace_members.iter().collect();
    let mut packages: Vec<&Package> = metadata
        .packages
        .iter()
        .filter(|package| member_ids.contains(&package.id))
        .collect();
    // Deterministic order regardless of metadata ordering (spec §75).
    packages.sort_by(|a, b| a.name.cmp(&b.name));

    let root = workspace_root.as_std_path();
    let name = root.file_name().map_or_else(
        || workspace_root.to_string(),
        |name| name.to_string_lossy().to_string(),
    );

    let project = Project {
        name: name.clone(),
        root: workspace_root.to_string(),
        crates: packages
            .iter()
            .map(|package| map_package(package, root))
            .collect(),
        workloads: Vec::new(),
    };

    let repository = Repository {
        name,
        projects: vec![project],
    };
    Ok((repository, root.to_path_buf()))
}

/// Maps one cargo package onto [`CrateInfo`].
fn map_package(package: &Package, workspace_root: &Path) -> CrateInfo {
    let has_target = |wanted: &TargetKind| {
        package
            .targets
            .iter()
            .any(|target| target.kind.iter().any(|kind| kind == wanted))
    };
    let has_build_script = package
        .targets
        .iter()
        .any(|target| target.kind.contains(&TargetKind::CustomBuild));

    let mut dependencies: Vec<DependencyInfo> = Vec::new();
    let mut uses_criterion = false;
    for dependency in &package.dependencies {
        if matches!(dependency.kind, CargoDepKind::Unknown) {
            continue;
        }
        let kind = match dependency.kind {
            CargoDepKind::Normal | CargoDepKind::Unknown => DependencyKind::Normal,
            CargoDepKind::Build => DependencyKind::Build,
            CargoDepKind::Development => DependencyKind::Dev,
        };
        let internal = dependency
            .path
            .as_ref()
            .is_some_and(|path| path.as_std_path().starts_with(workspace_root));
        uses_criterion |= is_runtime_criterion(&dependency.name, kind);
        dependencies.push(DependencyInfo {
            name: dependency.name.clone(),
            version_req: dependency.req.to_string(),
            kind,
            optional: dependency.optional,
            internal,
        });
    }
    dependencies.sort_by(|a, b| (&a.kind, &a.name).cmp(&(&b.kind, &b.name)));

    let mut features: Vec<String> = package.features.keys().cloned().collect();
    features.sort();

    let mut binaries: Vec<String> = package
        .targets
        .iter()
        .filter(|target| target.kind.contains(&TargetKind::Bin))
        .map(|target| target.name.clone())
        .collect();
    binaries.sort();

    CrateInfo {
        name: package.name.clone(),
        version: package.version.to_string(),
        manifest_path: package.manifest_path.to_string(),
        modules: Vec::new(), // filled by the static analysis engine (spec §9)
        dependencies,
        has_benches: has_target(&TargetKind::Bench),
        has_tests: has_target(&TargetKind::Test),
        has_examples: has_target(&TargetKind::Example),
        has_build_script,
        has_library: has_target(&TargetKind::Lib),
        binaries,
        features,
        uses_criterion,
    }
}

/// Criterion as a *runtime* dependency is a benchmark signal; as a
/// dev-dependency it is test-only and does not count (spec §8.3).
fn is_runtime_criterion(name: &str, kind: DependencyKind) -> bool {
    name == "criterion" && kind != DependencyKind::Dev
}

/// Walks `root` for Rust source files, respecting `.gitignore` and hidden
/// files (spec §7, §72: never descend into ignored/irrelevant paths).
pub fn source_files(root: &Path) -> impl Iterator<Item = PathBuf> {
    ignore::WalkBuilder::new(root)
        .hidden(true)
        // Fixture projects and non-git trees still get their .gitignore
        // honoured; git repos behave identically.
        .require_git(false)
        .build()
        .filter_map(|entry| {
            let path = entry.ok()?.into_path();
            (path.is_file() && path.extension().is_some_and(|ext| ext == "rs")).then_some(path)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn criterion_only_counts_as_runtime_dependency() {
        assert!(is_runtime_criterion("criterion", DependencyKind::Normal));
        assert!(is_runtime_criterion("criterion", DependencyKind::Build));
        assert!(!is_runtime_criterion("criterion", DependencyKind::Dev));
        assert!(!is_runtime_criterion("serde", DependencyKind::Normal));
    }

    #[test]
    fn source_walk_respects_gitignore() {
        let temp = std::env::temp_dir().join("tpt-discovery-walk-test");
        let src = temp.join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        std::fs::write(temp.join(".gitignore"), "generated*.rs\n").expect("gitignore");
        std::fs::write(src.join("real.rs"), "pub fn a() {}\n").expect("file");
        std::fs::write(src.join("generated_probe.rs"), "pub fn b() {}\n").expect("file");

        let mut found: Vec<PathBuf> = source_files(&temp).collect();
        found.sort();
        assert_eq!(found.len(), 1, "gitignored file must be skipped: {found:?}");
        assert!(found[0].ends_with("real.rs"));

        std::fs::remove_dir_all(&temp).expect("cleanup");
    }
}
