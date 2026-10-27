//! GitHub clone → analyse → store → confirm-delete (spec §70, Phase 2.8a).
//!
//! Every network step is explicit: the caller names an owner, and nothing
//! is cloned or listed otherwise. The steps are:
//!
//! 1. list the owner's repositories with `gh` and filter them (forks,
//!    archived, non-Rust, include and exclude globs);
//! 2. ask each repository for its head commit with `git ls-remote`, and
//!    reuse any stored result for that commit, so a rerun resumes;
//! 3. shallow-clone the rest into a managed cache directory, in batches
//!    whose estimated size fits a disk budget, and analyse each batch;
//! 4. store each result under `.tpt-profiler/github/`, keyed by repository
//!    and commit, independent of the clone;
//! 5. offer to delete the clones, but only directories this tool created
//!    (they carry a marker file and sit directly under the cache root).
//!
//! Authentication is whatever `gh` and `git` already have; no token is read
//! or stored here. The [`Host`] trait isolates the two commands so the
//! workflow can be tested with a fake host.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tpt_profiler_model::error::SerializationError;
use tpt_profiler_model::{Error, SchemaEnvelope};

use crate::baseline::STATE_DIR;
use crate::ecosystem;

/// Subdirectory of `.tpt-profiler/` holding stored per-repository results.
pub const STORE_DIR: &str = "github";

/// Subdirectory of `.tpt-profiler/` holding clones by default.
pub const CLONE_DIR: &str = "clones";

/// Marker written inside every clone this tool creates.
pub const MARKER_FILE: &str = ".tpt-profiler-clone";

/// Contents of [`MARKER_FILE`].
const MARKER_TEXT: &str = "Created by tpt-profiler ecosystem --github. Safe to delete.\n";

/// Disk budget for clones on disk at once, in kilobytes (2 GiB).
pub const DEFAULT_DISK_BUDGET_KB: u64 = 2 * 1024 * 1024;

/// Size assumed for a repository whose size GitHub does not report, in
/// kilobytes (20 MiB).
const DEFAULT_ESTIMATE_KB: u64 = 20 * 1024;

/// Upper bound for one `gh` or `git` network call.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(300);

/// Repositories requested from `gh repo list` before filtering.
const LIST_LIMIT: usize = 1000;

/// One repository as `gh repo list` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    /// `owner/name`.
    pub name_with_owner: String,
    /// Clone URL.
    pub url: String,
    /// Whether the repository is a fork.
    #[serde(default)]
    pub is_fork: bool,
    /// Whether the repository is archived.
    #[serde(default)]
    pub is_archived: bool,
    /// The main language, when GitHub detects one.
    #[serde(default)]
    pub primary_language: Option<Language>,
    /// Repository size in kilobytes, as GitHub reports it.
    #[serde(default)]
    pub disk_usage: Option<u64>,
}

/// A language name as `gh` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Language {
    /// Language name, such as `Rust`.
    pub name: String,
}

/// Why a repository was left out before cloning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// A fork of another repository.
    Fork,
    /// Archived on GitHub.
    Archived,
    /// Not detected as Rust.
    NotRust,
    /// Matched an exclude glob.
    Excluded,
    /// Did not match any include glob.
    NotIncluded,
}

impl Skip {
    /// Short reason for reports.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Fork => "fork",
            Self::Archived => "archived",
            Self::NotRust => "not Rust",
            Self::Excluded => "matched --exclude",
            Self::NotIncluded => "did not match --include",
        }
    }
}

/// Include and exclude globs over `owner/name`, or over `name` alone when
/// the pattern has no slash. `*` matches any run of characters and `?` one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoFilter {
    /// Repositories must match one of these when any are given.
    pub include: Vec<String>,
    /// Repositories matching any of these are skipped.
    pub exclude: Vec<String>,
}

/// Matches `text` against a glob with `*` and `?`.
#[must_use]
pub fn glob_match(pattern: &str, text: &str) -> bool {
    fn matches(pattern: &[char], text: &[char]) -> bool {
        match pattern.split_first() {
            None => text.is_empty(),
            Some(('*', rest)) => (0..=text.len()).any(|skip| matches(rest, &text[skip..])),
            Some(('?', rest)) => !text.is_empty() && matches(rest, &text[1..]),
            Some((literal, rest)) => text.first() == Some(literal) && matches(rest, &text[1..]),
        }
    }
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    matches(&pattern, &text)
}

/// Whether a glob applies to a repository. A pattern with a slash is matched
/// against `owner/name`; one without is matched against the name alone.
fn glob_applies(pattern: &str, name_with_owner: &str) -> bool {
    if pattern.contains('/') {
        glob_match(pattern, name_with_owner)
    } else {
        let name = name_with_owner
            .rsplit('/')
            .next()
            .unwrap_or(name_with_owner);
        glob_match(pattern, name)
    }
}

/// Repositories chosen for cloning, and those skipped with a reason.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Selection {
    /// Repositories to clone, in the order GitHub listed them.
    pub chosen: Vec<RepoInfo>,
    /// Each skipped repository's name and reason.
    pub skipped: Vec<(String, Skip)>,
}

/// Applies the fork, archive, language and glob rules to a listing.
#[must_use]
pub fn select(repos: Vec<RepoInfo>, filter: &RepoFilter) -> Selection {
    let mut selection = Selection::default();
    for repo in repos {
        let reason = if repo.is_fork {
            Some(Skip::Fork)
        } else if repo.is_archived {
            Some(Skip::Archived)
        } else if repo
            .primary_language
            .as_ref()
            .is_none_or(|language| language.name != "Rust")
        {
            Some(Skip::NotRust)
        } else if filter
            .exclude
            .iter()
            .any(|pattern| glob_applies(pattern, &repo.name_with_owner))
        {
            Some(Skip::Excluded)
        } else if !filter.include.is_empty()
            && !filter
                .include
                .iter()
                .any(|pattern| glob_applies(pattern, &repo.name_with_owner))
        {
            Some(Skip::NotIncluded)
        } else {
            None
        };
        match reason {
            Some(reason) => selection.skipped.push((repo.name_with_owner, reason)),
            None => selection.chosen.push(repo),
        }
    }
    selection
}

/// Groups repositories, in order, into batches whose estimated sizes fit
/// `budget_kb`. A repository larger than the budget gets a batch to itself.
/// Returns indices into `sizes_kb`.
#[must_use]
pub fn plan_batches(sizes_kb: &[u64], budget_kb: u64) -> Vec<Vec<usize>> {
    let mut batches = Vec::new();
    let mut current = Vec::new();
    let mut used = 0_u64;
    for (index, &size) in sizes_kb.iter().enumerate() {
        if !current.is_empty() && used.saturating_add(size) > budget_kb {
            batches.push(std::mem::take(&mut current));
            used = 0;
        }
        current.push(index);
        used = used.saturating_add(size);
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

/// Whether one path component is safe to use as a directory name.
fn is_safe_component(part: &str) -> bool {
    !part.is_empty()
        && !part.starts_with('.')
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Directory-safe form of `owner/name`, such as `owner__name`. Rejects
/// anything that is not exactly two safe components.
///
/// # Errors
/// Returns a message when the name is not `owner/name` with safe parts.
pub fn repo_dir_name(name_with_owner: &str) -> Result<String, String> {
    let mut parts = name_with_owner.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(owner), Some(name), None) if is_safe_component(owner) && is_safe_component(name) => {
            Ok(format!("{owner}__{name}"))
        }
        _ => Err(format!("`{name_with_owner}` is not a safe owner/name")),
    }
}

/// Whether a commit id has the shape of a git object name.
fn is_commit_id(text: &str) -> bool {
    (40..=64).contains(&text.len()) && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// Path of the stored result for one repository at one commit.
///
/// # Errors
/// Fails when the name or commit is not safe to use in a file name.
pub fn store_path(
    store_root: &Path,
    name_with_owner: &str,
    commit: &str,
) -> Result<PathBuf, String> {
    if !is_commit_id(commit) {
        return Err(format!("`{commit}` is not a commit id"));
    }
    let dir = store_root.join(STATE_DIR).join(STORE_DIR);
    Ok(dir.join(format!("{}@{commit}.json", repo_dir_name(name_with_owner)?)))
}

/// Path of the clone for one repository under `cache_root`.
///
/// # Errors
/// Fails when the name is not `owner/name` with safe parts.
pub fn clone_path(cache_root: &Path, name_with_owner: &str) -> Result<PathBuf, String> {
    Ok(cache_root.join(repo_dir_name(name_with_owner)?))
}

/// Whether `path` is a clone this tool created: a real directory (not a
/// symlink) directly under `cache_root` that holds [`MARKER_FILE`].
#[must_use]
pub fn is_managed_clone(cache_root: &Path, path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return false;
    }
    let (Some(parent), Ok(root)) = (path.parent(), fs::canonicalize(cache_root)) else {
        return false;
    };
    fs::canonicalize(parent).is_ok_and(|parent| parent == root) && path.join(MARKER_FILE).is_file()
}

/// Deletes a clone, refusing anything [`is_managed_clone`] does not accept.
///
/// # Errors
/// Fails when the path is not a managed clone or deletion fails.
pub fn remove_managed_clone(cache_root: &Path, path: &Path) -> Result<(), String> {
    if !is_managed_clone(cache_root, path) {
        return Err(format!(
            "refusing to delete `{}`: not a clone created by tpt-profiler",
            path.display()
        ));
    }
    fs::remove_dir_all(path)
        .map_err(|error| format!("could not delete `{}`: {error}", path.display()))
}

/// Managed clones under `cache_root` with their sizes in kilobytes, sorted
/// by name. A missing cache root has no clones.
#[must_use]
pub fn list_managed_clones(cache_root: &Path) -> Vec<(PathBuf, u64)> {
    let Ok(entries) = fs::read_dir(cache_root) else {
        return Vec::new();
    };
    let mut clones: Vec<(PathBuf, u64)> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_managed_clone(cache_root, path))
        .map(|path| {
            let size = dir_size_kb(&path);
            (path, size)
        })
        .collect();
    clones.sort();
    clones
}

/// Size of a directory tree in kilobytes, rounded up, not following links.
#[must_use]
pub fn dir_size_kb(path: &Path) -> u64 {
    fn bytes(path: &Path) -> u64 {
        let Ok(entries) = fs::read_dir(path) else {
            return 0;
        };
        entries
            .filter_map(Result::ok)
            .map(|entry| {
                let Ok(metadata) = entry.path().symlink_metadata() else {
                    return 0;
                };
                if metadata.is_dir() {
                    bytes(&entry.path())
                } else if metadata.is_file() {
                    metadata.len()
                } else {
                    0
                }
            })
            .sum()
    }
    bytes(path).div_ceil(1024)
}

/// One opportunity as stored: enough to rank it across repositories.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredOpportunity {
    /// Category name, such as `Allocation`.
    pub category: String,
    /// Finding text.
    pub description: String,
    /// Priority, when rated.
    pub priority: Option<u8>,
}

/// What analysing one repository produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    /// Analysed; the opportunities may be empty.
    Analysed {
        /// Opportunities found across the repository's crates.
        opportunities: Vec<StoredOpportunity>,
    },
    /// The clone has no `Cargo.toml` anywhere it looked, so there is nothing
    /// to analyse.
    NoManifest,
}

/// A stored result for one repository at one commit. Failures are not
/// stored, so a later run retries them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredRepo {
    /// `owner/name`.
    pub name_with_owner: String,
    /// Commit that was analysed.
    pub commit: String,
    /// Unix seconds when the analysis ran.
    pub analysed_at: i64,
    /// What the analysis found.
    pub outcome: Outcome,
}

/// Writes a stored result. Overwrites a previous result for the same commit.
///
/// # Errors
/// Fails when the path is unsafe or the file cannot be written.
pub fn save_result(store_root: &Path, repo: &StoredRepo) -> Result<PathBuf, Error> {
    let path = store_path(store_root, &repo.name_with_owner, &repo.commit)
        .map_err(|message| Error::Serialization(SerializationError::EncodeFailed { message }))?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|error| encode_failure(dir, &error))?;
    }
    let envelope = SchemaEnvelope::new(
        format!("tpt-profiler {}", tpt_profiler_model::version()),
        repo,
    );
    let json = serde_json::to_string_pretty(&envelope).map_err(|error| {
        Error::Serialization(SerializationError::EncodeFailed {
            message: error.to_string(),
        })
    })?;
    fs::write(&path, json).map_err(|error| encode_failure(&path, &error))?;
    Ok(path)
}

fn encode_failure(path: &Path, error: &std::io::Error) -> Error {
    Error::Serialization(SerializationError::EncodeFailed {
        message: format!("could not write `{}`: {error}", path.display()),
    })
}

/// Loads the stored result for one repository at one commit, if any.
///
/// # Errors
/// Fails on an unsafe name, an unreadable or corrupt file, or a schema this
/// build cannot read.
pub fn load_result(
    store_root: &Path,
    name_with_owner: &str,
    commit: &str,
) -> Result<Option<StoredRepo>, Error> {
    let path = store_path(store_root, name_with_owner, commit)
        .map_err(|message| Error::Serialization(SerializationError::DecodeFailed { message }))?;
    if !path.is_file() {
        return Ok(None);
    }
    read_stored(&path).map(Some)
}

fn read_stored(path: &Path) -> Result<StoredRepo, Error> {
    let text = fs::read_to_string(path).map_err(|error| {
        Error::Serialization(SerializationError::DecodeFailed {
            message: format!("could not read `{}`: {error}", path.display()),
        })
    })?;
    let envelope: SchemaEnvelope<StoredRepo> = serde_json::from_str(&text).map_err(|error| {
        Error::Serialization(SerializationError::DecodeFailed {
            message: format!("`{}` is corrupt: {error}", path.display()),
        })
    })?;
    envelope.validate().map_err(Error::Serialization)?;
    Ok(envelope.data)
}

/// Every stored result for one owner, sorted by repository then commit.
/// This reads only the store, so reports can be regenerated without the
/// source present.
///
/// # Errors
/// Fails when the owner name is unsafe or a stored file is unreadable.
pub fn load_owner_results(store_root: &Path, owner: &str) -> Result<Vec<StoredRepo>, Error> {
    if !is_safe_component(owner) {
        return Err(Error::Serialization(SerializationError::DecodeFailed {
            message: format!("`{owner}` is not a safe owner name"),
        }));
    }
    let dir = store_root.join(STATE_DIR).join(STORE_DIR);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let prefix = format!("{owner}__");
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix))
                && path
                    .extension()
                    .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort();
    paths.iter().map(|path| read_stored(path)).collect()
}

/// Access to `gh` and `git`. The real implementation is [`GitHost`].
pub trait Host: Sync {
    /// Lists an owner's repositories, up to `limit`.
    ///
    /// # Errors
    /// Returns the tool's message when listing fails.
    fn list_repos(&self, owner: &str, limit: usize) -> Result<Vec<RepoInfo>, String>;

    /// The commit id at the head of the default branch, without cloning.
    ///
    /// # Errors
    /// Returns the tool's message when the remote cannot be read.
    fn head_commit(&self, url: &str) -> Result<String, String>;

    /// Shallow-clones `url` into `destination`, which does not yet exist.
    ///
    /// # Errors
    /// Returns the tool's message when the clone fails.
    fn clone_shallow(&self, url: &str, destination: &Path) -> Result<(), String>;
}

/// [`Host`] backed by the `gh` and `git` executables on `PATH`.
#[derive(Debug, Default, Clone, Copy)]
pub struct GitHost;

/// Runs a command with the network timeout and returns its stdout.
fn run_tool(tool: &str, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new(tool);
    command.args(args);
    let outcome = tpt_profiler_runtime::process::run(&mut command, NETWORK_TIMEOUT, None).map_err(
        |error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                format!("`{tool}` is not on PATH; install it and log in before using --github")
            } else {
                error.to_string()
            }
        },
    )?;
    if outcome.timed_out {
        return Err(format!("`{tool} {}` timed out", args.join(" ")));
    }
    if !outcome.success {
        let stderr = outcome.stderr.trim();
        if stderr.to_ascii_lowercase().contains("rate limit") {
            return Err(String::from(
                "GitHub rate limit reached; wait and rerun, stored results are reused",
            ));
        }
        return Err(format!("`{tool}` failed: {stderr}"));
    }
    Ok(outcome.stdout)
}

impl Host for GitHost {
    fn list_repos(&self, owner: &str, limit: usize) -> Result<Vec<RepoInfo>, String> {
        let limit = limit.to_string();
        let fields = "nameWithOwner,url,isFork,isArchived,primaryLanguage,diskUsage";
        let stdout = run_tool(
            "gh",
            &["repo", "list", owner, "--limit", &limit, "--json", fields],
        )?;
        serde_json::from_str(&stdout).map_err(|error| format!("unexpected `gh` output: {error}"))
    }

    fn head_commit(&self, url: &str) -> Result<String, String> {
        let stdout = run_tool("git", &["ls-remote", url, "HEAD"])?;
        stdout
            .split_whitespace()
            .next()
            .filter(|commit| is_commit_id(commit))
            .map(str::to_owned)
            .ok_or_else(|| format!("`git ls-remote {url}` reported no HEAD commit"))
    }

    fn clone_shallow(&self, url: &str, destination: &Path) -> Result<(), String> {
        let destination = destination.to_string_lossy();
        run_tool(
            "git",
            &["clone", "--depth", "1", "--quiet", url, &destination],
        )
        .map(|_| ())
    }
}

/// What to do when clones would exceed the disk budget, and at the end of a
/// run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupPolicy {
    /// Delete clones without asking.
    Yes,
    /// Keep clones; stop when the budget would be exceeded.
    Keep,
    /// Ask through the confirmation callback; when it declines, keep.
    Ask,
}

/// Inputs for one GitHub run.
pub struct GithubOptions<'a> {
    /// Owner whose repositories are listed.
    pub owner: String,
    /// Include and exclude globs.
    pub filter: RepoFilter,
    /// Most repositories to consider after filtering.
    pub max_repos: usize,
    /// Workspace root; results are stored under its `.tpt-profiler/`.
    pub store_root: PathBuf,
    /// Directory holding managed clones.
    pub cache_root: PathBuf,
    /// Most kilobytes of clones on disk at once.
    pub disk_budget_kb: u64,
    /// Clones made at once.
    pub workers: usize,
    /// Cleanup behaviour.
    pub cleanup: CleanupPolicy,
    /// Asked with `(clone count, kilobytes)` when [`CleanupPolicy::Ask`].
    pub confirm: &'a dyn Fn(usize, u64) -> bool,
}

impl fmt::Debug for GithubOptions<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GithubOptions")
            .field("owner", &self.owner)
            .field("filter", &self.filter)
            .field("max_repos", &self.max_repos)
            .field("store_root", &self.store_root)
            .field("cache_root", &self.cache_root)
            .field("disk_budget_kb", &self.disk_budget_kb)
            .field("workers", &self.workers)
            .field("cleanup", &self.cleanup)
            .finish_non_exhaustive()
    }
}

/// What a GitHub run did.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GithubRun {
    /// Repositories skipped before cloning, with the reason.
    pub skipped: Vec<(String, Skip)>,
    /// Results from this run and from earlier runs at the same commit.
    pub repos: Vec<StoredRepo>,
    /// How many results were reused from the store rather than re-analysed.
    pub resumed: usize,
    /// Repositories that failed, with the reason. Failures are not stored.
    pub failures: Vec<(String, String)>,
    /// Set when the run stopped before finishing because the budget was
    /// reached and clones were kept.
    pub stopped_early: Option<String>,
    /// Managed clones still on disk at the end of the run.
    pub remaining_clones: Vec<(PathBuf, u64)>,
}

/// Lists, filters, clones, analyses and stores one owner's repositories.
///
/// # Errors
/// Fails when the owner cannot be listed or the store cannot be written.
/// Per-repository failures are recorded in [`GithubRun::failures`] instead.
pub fn run(host: &dyn Host, options: &GithubOptions<'_>) -> Result<GithubRun, String> {
    let listed = host.list_repos(&options.owner, LIST_LIMIT)?;
    let mut selection = select(listed, &options.filter);
    selection.chosen.truncate(options.max_repos);
    let mut run = GithubRun {
        skipped: selection.skipped,
        ..GithubRun::default()
    };

    // Resolve commits first: cheap, and it decides what needs cloning.
    let mut pending: Vec<(RepoInfo, String)> = Vec::new();
    for repo in selection.chosen {
        match host.head_commit(&repo.url) {
            Ok(commit) => match load_result(&options.store_root, &repo.name_with_owner, &commit) {
                Ok(Some(stored)) => {
                    run.resumed += 1;
                    run.repos.push(stored);
                }
                Ok(None) => pending.push((repo, commit)),
                Err(error) => run.failures.push((repo.name_with_owner, error.to_string())),
            },
            Err(error) => run.failures.push((repo.name_with_owner, error)),
        }
    }

    let sizes: Vec<u64> = pending
        .iter()
        .map(|(repo, _)| repo.disk_usage.unwrap_or(DEFAULT_ESTIMATE_KB))
        .collect();
    let batches = plan_batches(&sizes, options.disk_budget_kb);
    for batch in batches {
        let needed: u64 = batch.iter().map(|&index| sizes[index]).sum();
        let on_disk: u64 = list_managed_clones(&options.cache_root)
            .iter()
            .map(|(_, size)| size)
            .sum();
        if on_disk > 0 && on_disk.saturating_add(needed) > options.disk_budget_kb {
            let count = list_managed_clones(&options.cache_root).len();
            let approved = match options.cleanup {
                CleanupPolicy::Yes => true,
                CleanupPolicy::Keep => false,
                CleanupPolicy::Ask => (options.confirm)(count, on_disk),
            };
            if !approved {
                run.stopped_early = Some(format!(
                    "disk budget of {} MB reached with {count} clones kept; rerun with --yes to delete them, or raise --disk-budget-mb",
                    options.disk_budget_kb / 1024
                ));
                break;
            }
            delete_all_clones(&options.cache_root, &mut run.failures);
        }

        let batch_repos: Vec<&(RepoInfo, String)> =
            batch.iter().map(|&index| &pending[index]).collect();
        for chunk in batch_repos.chunks(options.workers.max(1)) {
            for (name, result) in clone_chunk(host, &options.cache_root, chunk) {
                match result {
                    Ok(stored) => {
                        save_result(&options.store_root, &stored)
                            .map_err(|error| error.to_string())?;
                        run.repos.push(stored);
                    }
                    Err(error) => run.failures.push((name, error)),
                }
            }
        }
    }

    if run.stopped_early.is_none() && options.cleanup != CleanupPolicy::Keep {
        let leftovers = list_managed_clones(&options.cache_root);
        let total: u64 = leftovers.iter().map(|(_, size)| size).sum();
        let approved = match options.cleanup {
            CleanupPolicy::Yes => true,
            CleanupPolicy::Ask => {
                !leftovers.is_empty() && (options.confirm)(leftovers.len(), total)
            }
            CleanupPolicy::Keep => false,
        };
        if approved {
            delete_all_clones(&options.cache_root, &mut run.failures);
        }
    }
    run.remaining_clones = list_managed_clones(&options.cache_root);
    run.repos
        .sort_by(|a, b| a.name_with_owner.cmp(&b.name_with_owner));
    Ok(run)
}

/// Clones and analyses one chunk of repositories on scoped threads, in
/// the order given. A panicking worker is reported as a failure.
fn clone_chunk(
    host: &dyn Host,
    cache_root: &Path,
    chunk: &[&(RepoInfo, String)],
) -> Vec<(String, Result<StoredRepo, String>)> {
    std::thread::scope(|scope| {
        let handles: Vec<(String, _)> = chunk
            .iter()
            .map(|(repo, commit)| {
                let name = repo.name_with_owner.clone();
                let handle = scope.spawn(move || clone_and_analyse(host, cache_root, repo, commit));
                (name, handle)
            })
            .collect();
        handles
            .into_iter()
            .map(|(name, handle)| {
                let result = handle
                    .join()
                    .unwrap_or_else(|_| Err(String::from("worker panicked")));
                (name, result)
            })
            .collect()
    })
}

/// Deletes every managed clone, recording any that could not be removed.
fn delete_all_clones(cache_root: &Path, failures: &mut Vec<(String, String)>) {
    for (path, _) in list_managed_clones(cache_root) {
        if let Err(error) = remove_managed_clone(cache_root, &path) {
            failures.push((path.display().to_string(), error));
        }
    }
}

/// Clones one repository, analyses it, and removes the clone again if the
/// clone or analysis failed, so a broken repository leaves nothing behind.
fn clone_and_analyse(
    host: &dyn Host,
    cache_root: &Path,
    repo: &RepoInfo,
    commit: &str,
) -> Result<StoredRepo, String> {
    let destination = clone_path(cache_root, &repo.name_with_owner)?;
    if destination.exists() {
        // Only ever replace something this tool made. Anything else is left
        // alone and the repository is reported as failed.
        if !is_managed_clone(cache_root, &destination) {
            return Err(format!(
                "`{}` exists and is not a clone created by tpt-profiler",
                destination.display()
            ));
        }
        remove_managed_clone(cache_root, &destination)?;
    }
    fs::create_dir_all(cache_root)
        .map_err(|error| format!("could not create `{}`: {error}", cache_root.display()))?;
    host.clone_shallow(&repo.url, &destination)?;
    fs::write(destination.join(MARKER_FILE), MARKER_TEXT)
        .map_err(|error| format!("could not mark `{}`: {error}", destination.display()))?;

    let outcome = match analyse_clone(&destination) {
        Ok(outcome) => outcome,
        Err(error) => {
            let _ = remove_managed_clone(cache_root, &destination);
            return Err(error);
        }
    };
    Ok(StoredRepo {
        name_with_owner: repo.name_with_owner.clone(),
        commit: commit.to_owned(),
        analysed_at: now_unix(),
        outcome,
    })
}

/// Analyses a clone. The root is analysed when it holds a `Cargo.toml`;
/// otherwise crates in subdirectories are found and analysed together.
fn analyse_clone(path: &Path) -> Result<Outcome, String> {
    let manifests = if path.join("Cargo.toml").is_file() {
        vec![path.to_path_buf()]
    } else {
        ecosystem::find_repositories(path, 3)
    };
    if manifests.is_empty() {
        return Ok(Outcome::NoManifest);
    }
    let mut opportunities = Vec::new();
    for manifest_dir in manifests {
        let found = ecosystem::analyse_repository(&manifest_dir)?;
        opportunities.extend(found.into_iter().map(|opportunity| {
            StoredOpportunity {
                category: opportunity.category.to_string(),
                description: opportunity.description,
                priority: opportunity
                    .priority
                    .map(tpt_profiler_model::Priority::value),
            }
        }));
    }
    Ok(Outcome::Analysed { opportunities })
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    fn repo(name: &str, language: Option<&str>) -> RepoInfo {
        RepoInfo {
            name_with_owner: format!("acme/{name}"),
            url: format!("https://example.invalid/acme/{name}.git"),
            is_fork: false,
            is_archived: false,
            primary_language: language.map(|name| Language {
                name: name.to_owned(),
            }),
            disk_usage: Some(100),
        }
    }

    #[test]
    fn globs_match_stars_and_question_marks() {
        assert!(glob_match("tpt-*", "tpt-pulse"));
        assert!(glob_match("t?t", "tpt"));
        assert!(!glob_match("tpt-*", "other-tpt"));
        assert!(glob_match("*", ""));
        assert!(glob_match("a*b*c", "axxbyyc"));
        assert!(!glob_match("a?c", "ac"));
    }

    #[test]
    fn slashless_globs_match_the_repository_name_only() {
        assert!(glob_applies("tpt-*", "acme/tpt-pulse"));
        assert!(!glob_applies("acme", "acme/tpt-pulse"));
        assert!(glob_applies("acme/tpt-*", "acme/tpt-pulse"));
        assert!(!glob_applies("other/*", "acme/tpt-pulse"));
    }

    #[test]
    fn selection_skips_forks_archives_non_rust_and_globs() {
        let mut fork = repo("forked", Some("Rust"));
        fork.is_fork = true;
        let mut archived = repo("old", Some("Rust"));
        archived.is_archived = true;
        let repos = vec![
            fork,
            archived,
            repo("python-thing", Some("Python")),
            repo("unknown", None),
            repo("tpt-keep", Some("Rust")),
            repo("scratch-test", Some("Rust")),
            repo("other", Some("Rust")),
        ];
        let filter = RepoFilter {
            include: vec![String::from("tpt-*"), String::from("scratch-*")],
            exclude: vec![String::from("*-test")],
        };
        let selection = select(repos, &filter);
        let chosen: Vec<&str> = selection
            .chosen
            .iter()
            .map(|repo| repo.name_with_owner.as_str())
            .collect();
        assert_eq!(chosen, ["acme/tpt-keep"]);
        let reasons: Vec<(String, Skip)> = selection.skipped;
        assert!(reasons.contains(&(String::from("acme/forked"), Skip::Fork)));
        assert!(reasons.contains(&(String::from("acme/old"), Skip::Archived)));
        assert!(reasons.contains(&(String::from("acme/python-thing"), Skip::NotRust)));
        assert!(reasons.contains(&(String::from("acme/unknown"), Skip::NotRust)));
        assert!(reasons.contains(&(String::from("acme/scratch-test"), Skip::Excluded)));
        assert!(reasons.contains(&(String::from("acme/other"), Skip::NotIncluded)));
    }

    #[test]
    fn batches_respect_the_budget_and_isolate_oversized_repositories() {
        assert_eq!(plan_batches(&[4, 4, 4], 8), vec![vec![0, 1], vec![2]]);
        assert_eq!(plan_batches(&[20, 1], 8), vec![vec![0], vec![1]]);
        assert!(plan_batches(&[], 8).is_empty());
    }

    #[test]
    fn unsafe_names_never_become_paths() {
        assert_eq!(repo_dir_name("acme/tpt-pulse").unwrap(), "acme__tpt-pulse");
        assert!(repo_dir_name("../etc").is_err());
        assert!(repo_dir_name("acme/..").is_err());
        assert!(repo_dir_name("acme/a/b").is_err());
        assert!(repo_dir_name("acme/.hidden").is_err());
        assert!(store_path(Path::new("x"), "acme/tpt", "../../evil").is_err());
    }

    fn temp(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("tpt-gh-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp root");
        root
    }

    fn managed(cache: &Path, name: &str) -> PathBuf {
        let dir = cache.join(name);
        fs::create_dir_all(dir.join("src")).expect("clone dir");
        fs::write(dir.join(MARKER_FILE), MARKER_TEXT).expect("marker");
        fs::write(dir.join("src").join("lib.rs"), "pub fn f() {}\n").expect("source");
        dir
    }

    #[test]
    fn only_marked_clones_directly_under_the_cache_are_deletable() {
        let root = temp("delete");
        let cache = root.join("cache");
        let clone = managed(&cache, "acme__tpt");
        let user_checkout = root.join("mine");
        fs::create_dir_all(&user_checkout).expect("checkout");
        fs::write(user_checkout.join("Cargo.toml"), "x").expect("file");
        let unmarked = cache.join("acme__plain");
        fs::create_dir_all(&unmarked).expect("unmarked");

        assert!(is_managed_clone(&cache, &clone));
        assert!(!is_managed_clone(&cache, &user_checkout));
        assert!(!is_managed_clone(&cache, &unmarked));
        assert!(!is_managed_clone(&cache, &cache));
        assert!(remove_managed_clone(&cache, &user_checkout).is_err());
        assert!(remove_managed_clone(&cache, &unmarked).is_err());
        assert!(user_checkout.exists());
        remove_managed_clone(&cache, &clone).expect("managed clone removed");
        assert!(!clone.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn stored_results_round_trip_and_reload_by_owner() {
        let root = temp("store");
        let commit = "a".repeat(40);
        let stored = StoredRepo {
            name_with_owner: String::from("acme/tpt-pulse"),
            commit: commit.clone(),
            analysed_at: 1,
            outcome: Outcome::Analysed {
                opportunities: vec![StoredOpportunity {
                    category: String::from("Allocation"),
                    description: String::from("vec in loop"),
                    priority: Some(42),
                }],
            },
        };
        save_result(&root, &stored).expect("saved");
        assert_eq!(
            load_result(&root, "acme/tpt-pulse", &commit).expect("loads"),
            Some(stored.clone())
        );
        assert_eq!(
            load_result(&root, "acme/tpt-pulse", &"b".repeat(40)).expect("loads"),
            None,
            "a new commit is not a stored result"
        );
        assert_eq!(
            load_owner_results(&root, "acme").expect("owner"),
            vec![stored]
        );
        assert!(load_owner_results(&root, "../x").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    /// A host that lists fixed repositories and clones from local fixture
    /// directories, so the workflow runs without `gh`, `git` or network.
    struct FakeHost {
        repos: Vec<RepoInfo>,
        fixtures: BTreeMap<String, PathBuf>,
        commits: BTreeMap<String, String>,
        clones: Mutex<Vec<String>>,
    }

    impl Host for FakeHost {
        fn list_repos(&self, _owner: &str, _limit: usize) -> Result<Vec<RepoInfo>, String> {
            Ok(self.repos.clone())
        }

        fn head_commit(&self, url: &str) -> Result<String, String> {
            self.commits
                .get(url)
                .cloned()
                .ok_or_else(|| format!("no commit for {url}"))
        }

        fn clone_shallow(&self, url: &str, destination: &Path) -> Result<(), String> {
            self.clones
                .lock()
                .map_err(|_| String::from("clone log poisoned"))?
                .push(url.to_owned());
            let fixture = self
                .fixtures
                .get(url)
                .ok_or_else(|| format!("no fixture for {url}"))?;
            copy_dir(fixture, destination).map_err(|error| error.to_string())
        }
    }

    fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let target = to.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_dir(&entry.path(), &target)?;
            } else {
                fs::copy(entry.path(), target)?;
            }
        }
        Ok(())
    }

    fn fixture(root: &Path, name: &str, source: &str) -> PathBuf {
        let dir = root.join("fixtures").join(name);
        fs::create_dir_all(dir.join("src")).expect("fixture dir");
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )
        .expect("manifest");
        fs::write(dir.join("src").join("lib.rs"), source).expect("source");
        dir
    }

    fn options(root: &Path, cleanup: CleanupPolicy, budget_kb: u64) -> GithubOptions<'static> {
        GithubOptions {
            owner: String::from("acme"),
            filter: RepoFilter::default(),
            max_repos: 10,
            store_root: root.join("workspace"),
            cache_root: root.join("workspace").join(STATE_DIR).join(CLONE_DIR),
            disk_budget_kb: budget_kb,
            workers: 2,
            cleanup,
            confirm: &|_, _| false,
        }
    }

    const ALLOCATING: &str =
        "pub fn f(n: usize) { for i in 0..n { let v = Vec::new(); drop(v + i); } }\n";

    #[test]
    fn run_clones_analyses_stores_and_resumes() {
        let root = temp("run");
        let fixtures = BTreeMap::from([
            (
                String::from("https://h/acme/alpha.git"),
                fixture(&root, "alpha", ALLOCATING),
            ),
            (
                String::from("https://h/acme/beta.git"),
                fixture(&root, "beta", "pub fn g() {}\n"),
            ),
        ]);
        let commits = BTreeMap::from([
            (String::from("https://h/acme/alpha.git"), "c".repeat(40)),
            (String::from("https://h/acme/beta.git"), "d".repeat(40)),
        ]);
        let host = FakeHost {
            repos: vec![
                RepoInfo {
                    url: String::from("https://h/acme/alpha.git"),
                    ..repo("alpha", Some("Rust"))
                },
                RepoInfo {
                    url: String::from("https://h/acme/beta.git"),
                    ..repo("beta", Some("Rust"))
                },
            ],
            fixtures,
            commits,
            clones: Mutex::new(Vec::new()),
        };
        let opts = options(&root, CleanupPolicy::Yes, DEFAULT_DISK_BUDGET_KB);

        let first = run(&host, &opts).expect("first run");
        assert_eq!(first.repos.len(), 2, "{first:?}");
        assert_eq!(first.resumed, 0);
        assert!(first.failures.is_empty(), "{:?}", first.failures);
        assert!(first.remaining_clones.is_empty(), "Yes deletes clones");
        assert!(matches!(
            first.repos.iter().find(|r| r.name_with_owner == "acme/alpha").map(|r| &r.outcome),
            Some(Outcome::Analysed { opportunities }) if !opportunities.is_empty()
        ));

        let second = run(&host, &opts).expect("second run");
        assert_eq!(second.resumed, 2, "same commits are reused from the store");
        assert_eq!(
            host.clones.lock().expect("log").len(),
            2,
            "nothing was cloned again"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_repository_without_a_manifest_is_stored_as_such() {
        let root = temp("nomanifest");
        let empty = root.join("fixtures").join("docs");
        fs::create_dir_all(&empty).expect("empty fixture");
        let host = FakeHost {
            repos: vec![RepoInfo {
                url: String::from("https://h/acme/docs.git"),
                ..repo("docs", Some("Rust"))
            }],
            fixtures: BTreeMap::from([(String::from("https://h/acme/docs.git"), empty)]),
            commits: BTreeMap::from([(String::from("https://h/acme/docs.git"), "e".repeat(40))]),
            clones: Mutex::new(Vec::new()),
        };
        let result = run(
            &host,
            &options(&root, CleanupPolicy::Yes, DEFAULT_DISK_BUDGET_KB),
        )
        .expect("run");
        assert_eq!(result.repos.len(), 1);
        assert_eq!(result.repos[0].outcome, Outcome::NoManifest);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_broken_repository_is_a_failure_and_the_run_goes_on() {
        let root = temp("broken");
        let good = fixture(&root, "good", ALLOCATING);
        let host = FakeHost {
            repos: vec![
                RepoInfo {
                    url: String::from("https://h/acme/good.git"),
                    ..repo("good", Some("Rust"))
                },
                RepoInfo {
                    url: String::from("https://h/acme/missing.git"),
                    ..repo("missing", Some("Rust"))
                },
            ],
            fixtures: BTreeMap::from([(String::from("https://h/acme/good.git"), good)]),
            commits: BTreeMap::from([
                (String::from("https://h/acme/good.git"), "f".repeat(40)),
                (String::from("https://h/acme/missing.git"), "0".repeat(40)),
            ]),
            clones: Mutex::new(Vec::new()),
        };
        let result = run(
            &host,
            &options(&root, CleanupPolicy::Yes, DEFAULT_DISK_BUDGET_KB),
        )
        .expect("run");
        assert_eq!(result.repos.len(), 1);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].0, "acme/missing");
        assert!(
            result.remaining_clones.is_empty(),
            "failed clone is removed"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn keep_stops_at_the_budget_instead_of_deleting() {
        let root = temp("budget");
        let host = FakeHost {
            repos: vec![
                RepoInfo {
                    url: String::from("https://h/acme/a.git"),
                    disk_usage: Some(60),
                    ..repo("a", Some("Rust"))
                },
                RepoInfo {
                    url: String::from("https://h/acme/b.git"),
                    disk_usage: Some(60),
                    ..repo("b", Some("Rust"))
                },
            ],
            fixtures: BTreeMap::from([
                (
                    String::from("https://h/acme/a.git"),
                    fixture(&root, "a", ALLOCATING),
                ),
                (
                    String::from("https://h/acme/b.git"),
                    fixture(&root, "b", ALLOCATING),
                ),
            ]),
            commits: BTreeMap::from([
                (String::from("https://h/acme/a.git"), "1".repeat(40)),
                (String::from("https://h/acme/b.git"), "2".repeat(40)),
            ]),
            clones: Mutex::new(Vec::new()),
        };
        // Each clone is estimated at 60 KB and the kept clone is only about
        // 1 KB on disk, so a 60 KB budget holds the second batch back.
        let result = run(&host, &options(&root, CleanupPolicy::Keep, 60)).expect("run");
        assert_eq!(result.repos.len(), 1, "the first batch finishes");
        assert!(
            result.stopped_early.is_some(),
            "the second batch is held back"
        );
        assert_eq!(result.remaining_clones.len(), 1, "the first clone is kept");
        let _ = fs::remove_dir_all(&root);
    }
}
