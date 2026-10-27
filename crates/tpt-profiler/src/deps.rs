//! Dependency-graph analysis (spec §18).
//!
//! Consumes `cargo metadata` and reports the dependency cost surface:
//! counts split into runtime vs compile-time vs dev-only cost
//! (spec §18's distinction), duplicated versions, feature surface, and
//! subtree size heuristics for "large dependency" detection. Each finding
//! carries a spec-§18 recommendation (reduction, feature reduction,
//! replacement, internalisation) as a *suggestion*, never a prescription.

use std::collections::{BTreeMap, BTreeSet};

use cargo_metadata::{DependencyKind, Metadata, Node, Package, PackageId};
use tpt_profiler_model::Confidence;

/// A dependency-analysis detection (spec §18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyFinding {
    /// Short title, e.g. `duplicate versions of clap`.
    pub title: String,
    /// Full description with the spec-§18 recommendation.
    pub description: String,
    /// Confidence in this heuristic (dependency heuristics cap at
    /// `Medium`).
    pub confidence: Confidence,
}

impl DependencyFinding {
    fn new(
        title: impl Into<String>,
        description: impl Into<String>,
        confidence: Confidence,
    ) -> Self {
        Self {
            title: title.into(),
            description: description.into(),
            confidence,
        }
    }
}

/// Aggregate dependency report (spec §18).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DependencyReport {
    /// Total packages in the resolved graph (direct + transitive).
    pub total_packages: usize,
    /// Direct `[dependencies]` (runtime cost).
    pub direct_normal: usize,
    /// Direct `[build-dependencies]` (compile-time cost).
    pub direct_build: usize,
    /// Direct `[dev-dependencies]` (test/bench-only cost).
    pub direct_dev: usize,
    /// `[target.'cfg(..)'.dependencies]` (platform-specific).
    pub target_gated: usize,
    /// Optional `[dependencies]`.
    pub optional: usize,
    /// Packages in the graph that no workspace member depends on
    /// directly (transitive).
    pub transitive_only: usize,
    /// Package names present with more than one distinct version.
    pub duplicates: Vec<DuplicateGroup>,
    /// Detections with recommendations (spec §18).
    pub findings: Vec<DependencyFinding>,
}

/// One package name with multiple resolved versions.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DuplicateGroup {
    /// Package name.
    pub name: String,
    /// All resolved versions, sorted.
    pub versions: Vec<String>,
}

/// Analyses the workspace dependency graph (spec §18).
#[must_use]
pub fn analyze(metadata: &Metadata) -> DependencyReport {
    let mut report = DependencyReport {
        total_packages: metadata.packages.len(),
        ..DependencyReport::default()
    };

    let member_ids: BTreeSet<&PackageId> = metadata.workspace_members.iter().collect();
    let mut direct_names: BTreeSet<String> = BTreeSet::new();
    let mut transitive_names: BTreeSet<String> = BTreeSet::new();

    for package in &metadata.packages {
        if member_ids.contains(&package.id) {
            for dependency in &package.dependencies {
                match dependency.kind {
                    DependencyKind::Normal => report.direct_normal += 1,
                    DependencyKind::Build => report.direct_build += 1,
                    DependencyKind::Development => report.direct_dev += 1,
                    DependencyKind::Unknown => {}
                }
                if dependency.target.is_some() {
                    report.target_gated += 1;
                }
                if dependency.optional {
                    report.optional += 1;
                }
                if matches!(
                    dependency.kind,
                    DependencyKind::Normal | DependencyKind::Build
                ) {
                    direct_names.insert(dependency.name.replace('-', "_"));
                }
            }
        } else {
            transitive_names.insert(package.name.replace('-', "_"));
        }
    }
    report.transitive_only = transitive_names
        .iter()
        .filter(|name| !direct_names.contains(*name))
        .count();

    // Duplicated versions across the whole graph (spec §18).
    report.duplicates = detect_duplicates(metadata);
    for duplicate in &report.duplicates {
        report.findings.push(DependencyFinding::new(
            format!("duplicate versions of {}", duplicate.name),
            format!(
                "{} resolves to {} versions ({}) — duplicated crates are compiled and \
                 audited twice; unify the version requirements or `cargo update` \
                 (spec §18: dependency reduction)",
                duplicate.name,
                duplicate.versions.len(),
                duplicate.versions.join(", ")
            ),
            Confidence::High,
        ));
    }

    // Compile-time cost concentration: build-dependencies that pull big
    // subtrees are pure compile-time cost (spec §18: distinguish runtime
    // from compile-time cost).
    if let Some(resolve) = &metadata.resolve {
        let subtrees = subtree_sizes(resolve);
        for node in &resolve.nodes {
            let Some(package) = package_of(metadata, &node.id) else {
                continue;
            };
            let size = subtrees.get(&node.id).copied().unwrap_or(0);
            if is_direct_build_dep(metadata, &node.id) && size >= 10 {
                report.findings.push(DependencyFinding::new(
                    format!("build-dependency {} pulls {} crates", package.name, size),
                    format!(
                        "build-dependency `{}` transitively pulls {size} unique crates — \
                         pure compile-time cost paid by every contributor and CI run; \
                         consider a lighter alternative or an internal implementation \
                         (spec §18: internal implementation)",
                        package.name
                    ),
                    Confidence::Medium,
                ));
            }

            // wasm32 compatibility (spec §21): crates that need an OS.
            if let Some(finding) = wasm_finding(&package.name, &node.features) {
                report.findings.push(finding);
            }

            // Feature surface heuristic (spec §18: feature usage).
            if node.features.len() >= 8 {
                report.findings.push(DependencyFinding::new(
                    format!(
                        "{} has {} features enabled",
                        package.name,
                        node.features.len()
                    ),
                    format!(
                        "`{}` is resolved with {} enabled features ({}); review `default-features` \
                         and enable only what is used — feature reduction shrinks both \
                         compile time and binary size (spec §18: feature reduction)",
                        package.name,
                        node.features.len(),
                        node.features.join(", ")
                    ),
                    Confidence::Low,
                ));
            }
        }
    }

    report.findings.sort_by(|a, b| a.title.cmp(&b.title));
    report
}

/// The spec-§21 finding for a resolved crate that cannot run on `wasm32`.
fn wasm_finding(name: &str, resolved: &[impl ToString]) -> Option<DependencyFinding> {
    let features: Vec<String> = resolved.iter().map(ToString::to_string).collect();
    let reason = wasm_unsupported(name, &features)?;
    Some(DependencyFinding::new(
        format!("{name} is not supported on wasm32"),
        format!(
            "`{name}` {reason}; a wasm32 build will fail or misbehave. Gate it with `cfg(not(target_arch = \"wasm32\"))` or pick a wasm-capable alternative              (spec §21: unsupported-dependency detection)"
        ),
        Confidence::Low,
    ))
}

/// Why a resolved crate does not build or run on `wasm32`, when it is known
/// not to. Matches a small, well-known set; anything else is assumed fine.
#[must_use]
pub fn wasm_unsupported(name: &str, features: &[String]) -> Option<&'static str> {
    let enabled = |wanted: &[&str]| {
        features
            .iter()
            .any(|feature| wanted.contains(&feature.as_str()))
    };
    match name {
        "rayon" => Some("needs OS threads, which wasm32 does not provide by default"),
        "mio" | "socket2" | "nix" => Some("wraps OS sockets or system calls"),
        "tokio" if enabled(&["net", "fs", "process", "signal", "rt-multi-thread"]) => {
            Some("enables tokio features that need an operating system")
        }
        _ => None,
    }
}

fn normalise_name(name: &str) -> String {
    name.replace('-', "_")
}

/// Groups packages that resolve to more than one version (spec §18).
fn detect_duplicates(metadata: &Metadata) -> Vec<DuplicateGroup> {
    let mut versions_by_name: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for package in &metadata.packages {
        versions_by_name
            .entry(normalise_name(&package.name))
            .or_default()
            .insert(package.version.to_string());
    }
    versions_by_name
        .into_iter()
        .filter(|(_, versions)| versions.len() > 1)
        .map(|(name, versions)| DuplicateGroup {
            name,
            versions: versions.into_iter().collect(),
        })
        .collect()
}

fn package_of<'a>(metadata: &'a Metadata, id: &PackageId) -> Option<&'a Package> {
    metadata.packages.iter().find(|package| &package.id == id)
}

fn is_direct_build_dep(metadata: &Metadata, id: &PackageId) -> bool {
    let target = normalise_name(&package_name(metadata, id));
    metadata.workspace_members.iter().any(|member| {
        package_of(metadata, member).is_some_and(|package| {
            package.dependencies.iter().any(|dependency| {
                dependency.kind == DependencyKind::Build
                    && normalise_name(&dependency.name) == target
            })
        })
    })
}

fn package_name(metadata: &Metadata, id: &PackageId) -> String {
    package_of(metadata, id).map_or_else(String::new, |package| package.name.to_string())
}

/// Unique transitive subtree size per node (memoised DFS).
fn subtree_sizes(resolve: &cargo_metadata::Resolve) -> BTreeMap<PackageId, usize> {
    let mut memo: BTreeMap<PackageId, usize> = BTreeMap::new();
    let nodes: BTreeMap<&PackageId, &Node> =
        resolve.nodes.iter().map(|node| (&node.id, node)).collect();
    for node in &resolve.nodes {
        size_of(&node.id, &nodes, &mut memo);
    }
    memo
}

fn size_of(
    id: &PackageId,
    nodes: &BTreeMap<&PackageId, &Node>,
    memo: &mut BTreeMap<PackageId, usize>,
) -> usize {
    if let Some(&size) = memo.get(id) {
        return size;
    }
    // Guard against dependency cycles in the graph.
    memo.insert(id.clone(), 0);
    let mut total = 0;
    if let Some(node) = nodes.get(id) {
        let mut unique: BTreeSet<&PackageId> = BTreeSet::new();
        for dependency in &node.deps {
            unique.insert(&dependency.pkg);
        }
        for dependency_id in unique {
            total += 1 + size_of(dependency_id, nodes, memo);
        }
    }
    memo.insert(id.clone(), total);
    total
}

#[cfg(test)]
mod tests {
    #[test]
    fn wasm_unsupported_flags_os_bound_crates_only() {
        assert!(wasm_unsupported("rayon", &[]).is_some());
        assert!(wasm_unsupported("tokio", &["net".to_owned()]).is_some());
        assert!(wasm_unsupported("tokio", &["macros".to_owned()]).is_none());
        assert!(wasm_unsupported("serde", &[]).is_none());
    }

    use super::*;

    fn metadata_from(json: &str) -> Metadata {
        serde_json::from_str(json).expect("metadata json")
    }

    const FIXTURE: &str = r#"{
        "packages": [
            {
                "name": "app", "version": "0.1.0", "id": "app 0.1.0",
                "manifest_path": "/ws/app/Cargo.toml", "dependencies": [], "targets": [], "features": {}, "edition": "2021"
            },
            {
                "name": "serde", "version": "1.0.210", "id": "serde 1.0.210",
                "manifest_path": "/home/.cargo/serde/Cargo.toml", "dependencies": [], "targets": [], "features": {}, "edition": "2021"
            },
            {
                "name": "serde", "version": "1.0.190", "id": "serde 1.0.190",
                "manifest_path": "/home/.cargo/serde-old/Cargo.toml", "dependencies": [], "targets": [], "features": {}, "edition": "2021"
            }
        ],
        "workspace_members": ["app 0.1.0"],
        "resolve": null,
        "workspace_root": "/ws",
        "target_directory": "/ws/target",
        "version": 8
    }"#;

    #[test]
    fn duplicates_are_detected_from_metadata_json() {
        let metadata = metadata_from(FIXTURE);
        let report = analyze(&metadata);
        assert_eq!(report.total_packages, 3);
        assert_eq!(report.duplicates.len(), 1);
        let group = &report.duplicates[0];
        assert_eq!(group.name, "serde");
        assert_eq!(group.versions, vec!["1.0.190", "1.0.210"]);
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.title.contains("duplicate versions of serde"))
        );
    }

    #[test]
    fn duplicate_names_normalise_dashes_and_underscores() {
        let metadata = metadata_from(
            r#"{
            "packages": [
                { "name": "my-lib", "version": "0.1.0", "id": "my-lib 0.1.0",
                  "manifest_path": "/ws/x/Cargo.toml", "dependencies": [], "targets": [], "features": {}, "edition": "2021" },
                { "name": "my_lib", "version": "0.2.0", "id": "my_lib 0.2.0",
                  "manifest_path": "/ws/y/Cargo.toml", "dependencies": [], "targets": [], "features": {}, "edition": "2021" }
            ],
            "workspace_members": ["my-lib 0.1.0", "my_lib 0.2.0"],
            "resolve": null,
            "workspace_root": "/ws",
            "target_directory": "/ws/target",
            "version": 8
        }"#,
        );
        let report = analyze(&metadata);
        assert_eq!(
            report.duplicates.len(),
            1,
            "dashes and underscores are the same crate name: {:?}",
            report.duplicates
        );
        assert_eq!(report.duplicates[0].name, "my_lib");
        assert_eq!(report.duplicates[0].versions, vec!["0.1.0", "0.2.0"]);
    }

    #[test]
    fn clean_graph_has_no_findings() {
        let metadata = metadata_from(
            r#"{
            "packages": [
                { "name": "solo", "version": "0.1.0", "id": "solo 0.1.0",
                  "manifest_path": "/ws/solo/Cargo.toml", "dependencies": [], "targets": [], "features": {}, "edition": "2021" }
            ],
            "workspace_members": ["solo 0.1.0"],
            "resolve": null,
            "workspace_root": "/ws",
            "target_directory": "/ws/target",
            "version": 8
        }"#,
        );
        let report = analyze(&metadata);
        assert_eq!(report.total_packages, 1);
        assert_eq!(report.duplicates.len(), 0);
        assert_eq!(report.findings.len(), 0);
        assert_eq!(report.transitive_only, 0);
    }

    #[test]
    fn feature_surface_finding_text_has_no_stray_escapes() {
        // Nine resolved features trips the feature-surface heuristic; its
        // multi-line message must read as one clean sentence (no literal
        // backslashes or indentation from line continuations).
        let metadata = metadata_from(
            r#"{
                "packages": [
                    {
                        "name": "clap", "version": "4.5.0", "id": "clap 4.5.0",
                        "manifest_path": "/home/.cargo/clap/Cargo.toml", "dependencies": [], "targets": [], "features": {}, "edition": "2021"
                    }
                ],
                "workspace_members": [],
                "resolve": {
                    "nodes": [
                        {
                            "id": "clap 4.5.0", "dependencies": [], "deps": [],
                            "features": ["a", "b", "c", "d", "e", "f", "g", "h", "i"]
                        }
                    ],
                    "root": null
                },
                "workspace_root": "/ws",
                "target_directory": "/ws/target",
                "version": 8
            }"#,
        );
        let report = analyze(&metadata);
        let finding = report
            .findings
            .iter()
            .find(|finding| finding.title.contains("features enabled"))
            .expect("feature-surface finding");
        assert!(
            !finding.description.contains('\\'),
            "{}",
            finding.description
        );
        assert!(
            finding
                .description
                .contains("review `default-features` and enable only"),
            "{}",
            finding.description
        );
    }
}
