//! The project hierarchy discovered by scanning (spec §6, §7):
//! Repository → Project → Crate → Module → Function, plus dependencies
//! and workloads.

use alloc::string::String;
use alloc::vec::Vec;

use crate::location::SourceLocation;
use crate::workload::Workload;

/// A repository under analysis (spec §6). The root of the hierarchy.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Repository {
    /// Repository name (directory name or configured project name).
    pub name: String,
    /// Projects found in the repository (usually one; ecosystem mode
    /// fills many, spec §33).
    pub projects: Vec<Project>,
}

/// A Cargo workspace (or standalone crate) with its workloads (spec §6).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Project {
    /// Project/workspace name.
    pub name: String,
    /// Absolute or workspace-relative root path.
    pub root: String,
    /// Member crates.
    pub crates: Vec<CrateInfo>,
    /// Workloads configured for this project (spec §23).
    pub workloads: Vec<Workload>,
}

/// One crate discovered in a project (spec §7).
// The four target flags are independent discovery facts, not a state
// machine; a bitmask/enum would obscure the manifest mapping.
#[allow(
    clippy::struct_excessive_bools,
    reason = "benches/tests/examples/build-script presence are independent"
)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CrateInfo {
    /// Crate (package) name.
    pub name: String,
    /// Declared version.
    pub version: String,
    /// Path to the crate's `Cargo.toml`.
    pub manifest_path: String,
    /// Modules walked from the crate root.
    pub modules: Vec<ModuleInfo>,
    /// Dependencies (normal, build, dev).
    pub dependencies: Vec<DependencyInfo>,
    /// Has `benches/` or `[[bench]]` targets.
    pub has_benches: bool,
    /// Has `tests/` or `[[test]]` targets.
    pub has_tests: bool,
    /// Has `examples/` or `[[example]]` targets.
    pub has_examples: bool,
    /// Has a `build.rs` script.
    pub has_build_script: bool,
    /// Has a library target.
    pub has_library: bool,
    /// Binary target names (spec §7).
    pub binaries: Vec<String>,
    /// Declared feature names (spec §7).
    pub features: Vec<String>,
    /// Whether any dependency is `criterion` (spec §8.3: benchmark
    /// discovery).
    pub uses_criterion: bool,
}

/// A module within a crate (spec §6).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModuleInfo {
    /// Module path relative to the crate, e.g. `mdct::transform`.
    pub name: String,
    /// File backing the module.
    pub file: String,
    /// Functions found in the module.
    pub functions: Vec<FunctionInfo>,
}

/// A function within a module (spec §6).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FunctionInfo {
    /// Function name (without module path).
    pub name: String,
    /// Where the function is defined.
    pub location: SourceLocation,
}

/// Dependency kind (spec §7, §18). Ordered for deterministic sorting:
/// Normal < Build < Dev < Target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum DependencyKind {
    /// `[dependencies]` — runtime cost.
    Normal,
    /// `[build-dependencies]` — compile-time cost.
    Build,
    /// `[dev-dependencies]` — test/bench only.
    Dev,
    /// `[target.'cfg(...)'.dependencies]`.
    Target,
}

/// A declared dependency (spec §18).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DependencyInfo {
    /// Dependency (package) name.
    pub name: String,
    /// Version requirement as written in the manifest.
    pub version_req: String,
    /// Which dependency section declared it.
    pub kind: DependencyKind,
    /// Whether the dependency is optional.
    pub optional: bool,
    /// `true` for a path/workspace dependency of this workspace.
    pub internal: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn hierarchy_defaults_compose() {
        let repository = Repository::default();
        assert_eq!(repository.name, "");
        assert_eq!(repository.projects.len(), 0);

        let project = Project {
            name: String::from("tpt-cadence"),
            root: String::from("."),
            crates: vec![CrateInfo {
                name: String::from("cadence-core"),
                has_benches: true,
                ..CrateInfo::default()
            }],
            workloads: vec![Workload::named("decode_1080p")],
        };
        assert_eq!(project.crates.len(), 1);
        assert!(project.crates[0].has_benches);
        assert_eq!(project.workloads[0].name, "decode_1080p");
    }

    #[test]
    fn function_locations_round_trip() {
        let function = FunctionInfo {
            name: String::from("transform_block"),
            location: SourceLocation::at_line("src/mdct.rs", 142),
        };
        assert_eq!(function.location.line_start, 142);
    }
}
