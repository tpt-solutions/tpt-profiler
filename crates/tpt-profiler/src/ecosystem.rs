//! Ecosystem scanning (spec §33, §34, §60): analyse many Rust repositories
//! under one directory, then report what they have in common.
//!
//! The scan is static-only and local (spec §70): no clones, no network and
//! no workloads. Each repository is analysed on its own, so one broken
//! repository is recorded and the rest still run (spec §72).
//!
//! Memory stays bounded for hundreds of repositories: each analysis is
//! folded into small per-repository and per-opportunity summaries as soon
//! as it finishes, and the opportunities themselves are dropped.

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use serde::Serialize;
use tpt_profiler_model::Priority;

use crate::config::Config;
use crate::pipeline;

/// Directories never searched for repositories.
const SKIPPED_DIRECTORIES: &[&str] = &["target", "node_modules"];

/// Default depth below the scan root at which repositories are searched.
pub const DEFAULT_MAX_DEPTH: usize = 4;

/// Default extra weight per additional repository sharing an opportunity
/// (spec §60: shared infrastructure is worth more than one local fix).
pub const DEFAULT_SHARED_WEIGHT: f64 = 0.5;

/// Upper bound on default parallelism, so a large machine does not open
/// hundreds of cargo processes or files at once.
const MAX_DEFAULT_WORKERS: usize = 8;

/// Worker count used when none is given: the machine's parallelism, capped.
#[must_use]
pub fn default_workers() -> usize {
    std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(MAX_DEFAULT_WORKERS)
}

/// How the ecosystem scan is run.
#[derive(Debug, Clone, PartialEq)]
pub struct EcosystemOptions {
    /// How deep below the root to look for repositories.
    pub max_depth: usize,
    /// Score bonus per extra repository sharing an opportunity.
    pub shared_weight: f64,
    /// Repositories analysed at once (spec §72: bounded parallelism).
    pub workers: usize,
}

impl Default for EcosystemOptions {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
            shared_weight: DEFAULT_SHARED_WEIGHT,
            workers: default_workers(),
        }
    }
}

/// One repository's result in the ecosystem scan.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepositorySummary {
    /// Directory name of the repository.
    pub name: String,
    /// Path as found under the scan root.
    pub path: PathBuf,
    /// Opportunities detected in this repository.
    pub opportunities: usize,
    /// Highest priority among its opportunities, when any were rated.
    pub top_priority: Option<u8>,
    /// Why the repository could not be analysed, when it could not.
    pub error: Option<String>,
}

/// An opportunity found in more than one repository (spec §34).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SharedOpportunity {
    /// Category name (e.g. `Allocation`).
    pub category: String,
    /// The finding text, which is shared across repositories.
    pub description: String,
    /// Repositories where it was found, sorted.
    pub repositories: Vec<String>,
    /// Highest priority it reached in any repository.
    pub best_priority: Option<u8>,
    /// Ranking score: best priority, weighted up for each extra repository.
    pub score: f64,
    /// What to do about it: a shared primitive when it spans three or more
    /// repositories (spec §34), otherwise a fix in each.
    pub suggestion: String,
}

/// The whole ecosystem scan.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EcosystemReport {
    /// Root that was scanned.
    pub root: PathBuf,
    /// Every repository found, sorted by path.
    pub repositories: Vec<RepositorySummary>,
    /// Opportunities shared by at least two repositories, best first.
    pub shared: Vec<SharedOpportunity>,
}

/// Finds Rust repositories under `root`: directories holding a
/// `Cargo.toml`. Search stops at each repository, so crates inside a
/// workspace belong to that repository's analysis, not to the scan.
#[must_use]
pub fn find_repositories(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect(root, 0, max_depth, &mut found);
    found.sort();
    found
}

fn collect(dir: &Path, depth: usize, max_depth: usize, found: &mut Vec<PathBuf>) {
    if dir.join("Cargo.toml").is_file() {
        found.push(dir.to_path_buf());
        return;
    }
    if depth >= max_depth {
        return;
    }
    // An unreadable directory is skipped, not fatal: the scan goes on.
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| !name.starts_with('.') && !SKIPPED_DIRECTORIES.contains(&name))
        })
        .collect();
    children.sort();
    for child in children {
        collect(&child, depth + 1, max_depth, found);
    }
}

/// Accumulates one opportunity across the repositories it appears in.
#[derive(Debug, Default)]
struct Sharing {
    repositories: BTreeSet<String>,
    best_priority: Option<u8>,
}

/// What one repository contributed to the scan.
#[derive(Debug)]
struct Analysed {
    summary: RepositorySummary,
    /// Category, finding text and priority of each opportunity.
    findings: Vec<(String, String, Option<u8>)>,
}

/// Scans every repository under `root` (static analysis only) and reports
/// per-repository results and the opportunities they share. Repositories
/// are analysed on `options.workers` threads; results are merged in path
/// order, so the report does not depend on scheduling.
#[must_use]
pub fn scan(root: &Path, options: &EcosystemOptions) -> EcosystemReport {
    let paths = find_repositories(root, options.max_depth);
    let analysed = analyse_all(&paths, options.workers);

    let mut repositories = Vec::with_capacity(analysed.len());
    let mut findings = Vec::with_capacity(analysed.len());
    for repository in analysed {
        findings.push((repository.summary.name.clone(), repository.findings));
        repositories.push(repository.summary);
    }
    let shared = shared_from(findings, options.shared_weight);

    EcosystemReport {
        root: root.to_path_buf(),
        repositories,
        shared,
    }
}

/// Ranks opportunities found in more than one repository (spec §34, §60).
///
/// Each item is a repository name with its `(category, finding text,
/// priority)` findings. An opportunity is shared when at least two
/// repositories report the same category and text. Ranking is by best
/// priority, weighted up for each extra repository, so the same result comes
/// out whether the findings were just analysed or read back from a store.
#[must_use]
pub fn shared_from<I, F>(repositories: I, shared_weight: f64) -> Vec<SharedOpportunity>
where
    I: IntoIterator<Item = (String, F)>,
    F: IntoIterator<Item = (String, String, Option<u8>)>,
{
    let mut sharing: BTreeMap<(String, String), Sharing> = BTreeMap::new();
    for (name, findings) in repositories {
        for (category, description, priority) in findings {
            let entry = sharing.entry((category, description)).or_default();
            entry.repositories.insert(name.clone());
            entry.best_priority = entry.best_priority.max(priority);
        }
    }

    let mut shared: Vec<SharedOpportunity> = sharing
        .into_iter()
        .filter(|(_, entry)| entry.repositories.len() >= 2)
        .map(|((category, description), entry)| {
            let count = entry.repositories.len();
            let best = entry.best_priority.unwrap_or(0);
            SharedOpportunity {
                category,
                description,
                repositories: entry.repositories.into_iter().collect(),
                best_priority: entry.best_priority,
                score: f64::from(best) * (1.0 + shared_weight * extra_repositories(count)),
                suggestion: suggestion_for(count),
            }
        })
        .collect();
    shared.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.category.cmp(&b.category))
            .then_with(|| a.description.cmp(&b.description))
    });
    shared
}

/// Repositories beyond the first, as a weight for the shared score.
fn extra_repositories(count: usize) -> f64 {
    f64::from(u32::try_from(count.saturating_sub(1)).unwrap_or(u32::MAX))
}

/// Analyses `paths` on a pool of `workers` threads, returning results in
/// the same order as `paths`.
fn analyse_all(paths: &[PathBuf], workers: usize) -> Vec<Analysed> {
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::with_capacity(paths.len()));
    let threads = workers.clamp(1, paths.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(index) else {
                        break;
                    };
                    let analysed = analyse_guarded(path);
                    results
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push((index, analysed));
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap_or_else(PoisonError::into_inner);
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, analysed)| analysed).collect()
}

/// Analyses one repository, turning a panic into a recorded error so one
/// broken repository cannot take the whole scan down (spec §72).
fn analyse_guarded(path: &Path) -> Analysed {
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| analyse_repository(path)))
        .unwrap_or_else(|_| Err(String::from("analysis panicked")));
    let name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    match outcome {
        Ok(opportunities) => {
            let top_priority = opportunities
                .iter()
                .filter_map(|opportunity| opportunity.priority.map(Priority::value))
                .max();
            let findings = opportunities
                .iter()
                .map(|opportunity| {
                    (
                        opportunity.category.to_string(),
                        opportunity.description.clone(),
                        opportunity.priority.map(Priority::value),
                    )
                })
                .collect();
            Analysed {
                summary: RepositorySummary {
                    name,
                    path: path.to_path_buf(),
                    opportunities: opportunities.len(),
                    top_priority,
                    error: None,
                },
                findings,
            }
        }
        Err(error) => Analysed {
            summary: RepositorySummary {
                name,
                path: path.to_path_buf(),
                opportunities: 0,
                top_priority: None,
                error: Some(error),
            },
            findings: Vec::new(),
        },
    }
}

/// The recommendation for an opportunity found in `count` repositories.
fn suggestion_for(count: usize) -> String {
    if count >= 3 {
        format!(
            "found in {count} repositories: one shared helper or primitive in a common crate would fix all of them (spec §34)"
        )
    } else {
        String::from(
            "found in 2 repositories: fix each one, or extract a shared helper if the fix is the same",
        )
    }
}

/// Analyses one repository statically. Errors come back as text so the
/// scan can record them and move on. A manifest that is not valid TOML is
/// reported here, because the static path never asks cargo to read it.
pub(crate) fn analyse_repository(
    path: &Path,
) -> Result<Vec<tpt_profiler_model::Opportunity>, String> {
    let manifest = path.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|error| format!("could not read {}: {error}", manifest.display()))?;
    toml::from_str::<toml::Table>(&text)
        .map_err(|error| format!("{} is not valid TOML: {error}", manifest.display()))?;

    let config = Config::load_discovered(path)
        .map(|(config, _)| config)
        .unwrap_or_default();
    pipeline::analyse_static(path, &config)
        .map(|run| run.opportunities)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("tpt-eco-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }

    fn crate_at(path: &Path) {
        std::fs::create_dir_all(path.join("src")).expect("crate dir");
        std::fs::write(
            path.join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .expect("manifest");
        std::fs::write(path.join("src").join("lib.rs"), "pub fn f() {}\n").expect("source");
    }

    #[test]
    fn repositories_are_found_at_any_depth_within_the_limit() {
        let root = temp_root("find");
        crate_at(&root.join("alpha"));
        crate_at(&root.join("group").join("beta"));
        crate_at(&root.join("group").join("deep").join("gamma"));
        std::fs::create_dir_all(root.join("target").join("hidden")).expect("skipped dir");
        crate_at(&root.join("target").join("built"));

        let found = find_repositories(&root, 2);
        let names: Vec<String> = found
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["alpha", "beta"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn search_stops_at_a_repository_so_workspace_members_are_not_separate() {
        let root = temp_root("workspace");
        crate_at(&root.join("outer"));
        crate_at(&root.join("outer").join("member"));
        let found = find_repositories(&root, 4);
        assert_eq!(found, [root.join("outer")]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_broken_repository_is_recorded_and_the_scan_goes_on() {
        let root = temp_root("broken");
        crate_at(&root.join("good"));
        std::fs::create_dir_all(root.join("bad")).expect("dir");
        std::fs::write(root.join("bad").join("Cargo.toml"), "not = [valid").expect("manifest");

        let report = scan(&root, &EcosystemOptions::default());
        assert_eq!(report.repositories.len(), 2);
        let bad = report
            .repositories
            .iter()
            .find(|repo| repo.name == "bad")
            .expect("bad repo listed");
        assert!(bad.error.is_some());
        let good = report
            .repositories
            .iter()
            .find(|repo| repo.name == "good")
            .expect("good repo listed");
        assert!(good.error.is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Scale check for the 300+ repository target (spec §72). Slow by design,
    /// so it only runs on request: `cargo test -p tpt-profiler -- --ignored`.
    #[test]
    #[ignore = "scans 300 generated repositories; run with --ignored"]
    fn three_hundred_repositories_scan_and_aggregate() {
        let root = temp_root("scale");
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tpt-profiler-static/tests/corpus/hot_loop.rs");
        for index in 0..300 {
            let repo = root.join(format!("repo-{index:03}"));
            crate_at(&repo);
            std::fs::copy(&corpus, repo.join("src").join("lib.rs")).expect("corpus");
        }

        let started = std::time::Instant::now();
        let report = scan(&root, &EcosystemOptions::default());
        let elapsed = started.elapsed();
        eprintln!(
            "scanned {} repositories in {elapsed:?}",
            report.repositories.len()
        );

        assert_eq!(report.repositories.len(), 300);
        assert!(report.repositories.iter().all(|repo| repo.error.is_none()));
        assert!(!report.shared.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_same_finding_in_two_repositories_is_reported_as_shared() {
        let root = temp_root("shared");
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tpt-profiler-static/tests/corpus/hot_loop.rs");
        for name in ["first", "second"] {
            crate_at(&root.join(name));
            std::fs::copy(&corpus, root.join(name).join("src").join("lib.rs")).expect("corpus");
        }

        let report = scan(&root, &EcosystemOptions::default());
        assert!(!report.shared.is_empty(), "{report:?}");
        let shared = &report.shared[0];
        assert_eq!(shared.repositories, ["first", "second"]);
        let single = shared.best_priority.map_or(0.0, f64::from);
        assert!(shared.score >= single, "score is weighted up, never down");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn worker_count_does_not_change_the_report() {
        let root = temp_root("workers");
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tpt-profiler-static/tests/corpus/hot_loop.rs");
        for name in ["one", "two", "three"] {
            crate_at(&root.join(name));
            std::fs::copy(&corpus, root.join(name).join("src").join("lib.rs")).expect("corpus");
        }
        let serial = scan(
            &root,
            &EcosystemOptions {
                workers: 1,
                ..Default::default()
            },
        );
        let parallel = scan(
            &root,
            &EcosystemOptions {
                workers: 4,
                ..Default::default()
            },
        );
        assert_eq!(serial, parallel);
        let _ = std::fs::remove_dir_all(&root);
    }
}
