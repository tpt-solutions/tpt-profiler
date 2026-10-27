//! Terminal rendering of the discovery tree (spec §7).
//!
//! Output is deterministic (spec §75): crates are listed in sorted order,
//! and every line is plain text (colour is applied by the CLI layer, not
//! here).

use tpt_profiler_model::hierarchy::{CrateInfo, Repository};

/// Renders the `scan` tree output (spec §7):
///
/// ```text
/// Repository: multi-crate
/// └── Project: multi-crate (…/multi-crate)
///     ├── member-a 0.1.0 [lib, bench, test]
///     └── member-b 0.1.0 [lib]
/// ```
#[must_use]
pub fn render_scan(repository: &Repository) -> String {
    let mut out = String::new();
    out.push_str("Repository: ");
    out.push_str(&repository.name);
    out.push('\n');

    for (project_index, project) in repository.projects.iter().enumerate() {
        let project_last = project_index + 1 == repository.projects.len();
        out.push_str(if project_last {
            "└── "
        } else {
            "├── "
        });
        out.push_str("Project: ");
        out.push_str(&project.name);
        out.push_str(" (");
        out.push_str(&project.root);
        out.push_str(")\n");

        let mut crates: Vec<&CrateInfo> = project.crates.iter().collect();
        crates.sort_by(|a, b| a.name.cmp(&b.name));
        for (crate_index, info) in crates.iter().enumerate() {
            let crate_last = crate_index + 1 == crates.len();
            out.push_str(if project_last { "    " } else { "│   " });
            out.push_str(if crate_last {
                "└── "
            } else {
                "├── "
            });
            out.push_str(&info.name);
            out.push(' ');
            out.push_str(&info.version);
            out.push_str(" [");
            out.push_str(&target_summary(info));
            out.push_str("]\n");
        }
    }
    out
}

/// Compact target/flag summary: `lib`, `bin`, `bench`, `test`, `example`,
/// `build-script`, `criterion`, `features+name`.
fn target_summary(info: &CrateInfo) -> String {
    let mut parts: Vec<&str> = Vec::new();
    parts.push("lib"); // modelled crates always have a library or package root
    if info.has_benches {
        parts.push("bench");
    }
    if info.has_tests {
        parts.push("test");
    }
    if info.has_examples {
        parts.push("example");
    }
    if info.has_build_script {
        parts.push("build-script");
    }
    if info.uses_criterion {
        parts.push("criterion");
    }
    let mut summary = parts.join(", ");
    if !info.features.is_empty() {
        summary.push_str(", features+");
        summary.push_str(&info.features.join(","));
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_profiler_model::hierarchy::{CrateInfo, Project};

    fn sample() -> Repository {
        let mk = |name: &str| CrateInfo {
            name: String::from(name),
            version: String::from("0.1.0"),
            manifest_path: String::new(),
            modules: Vec::new(),
            dependencies: Vec::new(),
            has_benches: name == "a",
            has_tests: name == "a",
            has_examples: false,
            has_build_script: false,
            has_library: true,
            binaries: Vec::new(),
            features: Vec::new(),
            uses_criterion: false,
        };
        Repository {
            name: String::from("multi-crate"),
            projects: vec![Project {
                name: String::from("multi-crate"),
                root: String::from("/tmp/multi-crate"),
                crates: vec![mk("b"), mk("a")],
                workloads: Vec::new(),
            }],
        }
    }

    #[test]
    fn tree_is_sorted_and_deterministic() {
        let rendered = render_scan(&sample());
        let a = rendered.find("a 0.1.0").expect("a present");
        let b = rendered.find("b 0.1.0").expect("b present");
        assert!(a < b, "crates must render sorted: {rendered}");
        assert_eq!(rendered, render_scan(&sample()));
    }

    #[test]
    fn tree_shows_targets() {
        let rendered = render_scan(&sample());
        assert!(rendered.contains("[lib, bench, test]"), "{rendered}");
        assert!(rendered.contains("[lib]"), "{rendered}");
        assert!(rendered.starts_with("Repository: multi-crate\n"));
        assert!(rendered.contains("└── Project: multi-crate"));
    }

    #[test]
    fn multiple_projects_use_tree_glyphs() {
        let mut repository = sample();
        repository.projects.push(Project {
            name: String::from("second"),
            root: String::from("/tmp/second"),
            crates: vec![],
            workloads: Vec::new(),
        });
        let rendered = render_scan(&repository);
        assert!(rendered.contains("├── Project: multi-crate"), "{rendered}");
        assert!(rendered.contains("└── Project: second"), "{rendered}");
    }

    #[test]
    fn format_width_is_stable() {
        // Keep the golden shape of the output (spec §76 golden tests grow
        // from this seed).
        let rendered = render_scan(&sample());
        let expected_first = "Repository: multi-crate\n└── Project: multi-crate (/tmp/multi-crate)\n    ├── a 0.1.0 [lib, bench, test]\n    └── b 0.1.0 [lib]\n";
        assert_eq!(rendered, expected_first.to_string());
    }
}
