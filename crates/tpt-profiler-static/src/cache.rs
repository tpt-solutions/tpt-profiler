//! Incremental static-analysis cache (spec §72, Phase 2.9).
//!
//! Findings for a file are reused while the file's contents are unchanged
//! and the analyzer set is the same. The cache is keyed by a content hash,
//! never by timestamps, so it is correct after checkouts, rebases and
//! clock skew. It is a speed-up only: a missing, corrupt or stale cache is
//! ignored, and an unwritable one is not an error.
//!
//! Files that fail to parse are not cached, because they produce parse
//! errors that must be reported on every run.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tpt_profiler_model::{Category, Confidence, SourceLocation};

use crate::finding::Finding;

/// File name of the cache under the state directory.
pub const CACHE_FILE: &str = "static-cache.json";

/// Version of the cache file layout. A different value is treated as stale.
pub const FORMAT: u32 = 1;

/// The whole cache: which analyzers produced it, and what each file gave.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct StaticCache {
    /// Layout version (see [`FORMAT`]).
    pub format: u32,
    /// Analyzer and model-version key the findings were produced with.
    pub engine: String,
    /// Findings per source file, keyed by path relative to the scanned root.
    pub files: BTreeMap<String, CachedFile>,
}

/// One file's cached result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFile {
    /// FNV-1a hash of the file contents when it was analysed.
    pub hash: u64,
    /// The findings it produced.
    pub findings: Vec<CachedFinding>,
}

/// A finding in owned form. [`Finding`] holds a `&'static str` detector
/// name, which cannot be read back from a file, so it is stored as text
/// and interned on load.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedFinding {
    category: Category,
    detector: String,
    description: String,
    location: SourceLocation,
    confidence: Confidence,
    numerical_context: bool,
}

impl CachedFinding {
    /// Captures a finding for storage.
    #[must_use]
    pub fn from_finding(finding: &Finding) -> Self {
        Self {
            category: finding.category,
            detector: finding.detector.to_owned(),
            description: finding.description.clone(),
            location: finding.location.clone(),
            confidence: finding.confidence,
            numerical_context: finding.numerical_context,
        }
    }

    /// Rebuilds the finding it was captured from.
    #[must_use]
    pub fn into_finding(self) -> Finding {
        Finding {
            category: self.category,
            detector: intern(&self.detector),
            description: self.description,
            location: self.location,
            confidence: self.confidence,
            numerical_context: self.numerical_context,
        }
    }
}

/// Detector names are a small, fixed set, so each one is leaked once and
/// shared. This keeps [`Finding::detector`] `'static` without a copy per
/// finding.
fn intern(name: &str) -> &'static str {
    static NAMES: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());
    let mut names = NAMES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(existing) = names.get(name) {
        return existing;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    names.insert(leaked);
    leaked
}

/// FNV-1a over the file contents. Not cryptographic: it only decides
/// whether a cached entry is still valid for the same engine.
#[must_use]
pub fn hash_source(source: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    source.bytes().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

/// Loads the cache at `path` for the engine key `engine`. Any problem
/// (missing file, unreadable, corrupt, other format or other engine) gives
/// an empty cache, so the caller re-analyses everything.
#[must_use]
pub fn load(path: &Path, engine: &str) -> StaticCache {
    let fresh = || StaticCache {
        format: FORMAT,
        engine: engine.to_owned(),
        files: BTreeMap::new(),
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return fresh();
    };
    match serde_json::from_str::<StaticCache>(&text) {
        Ok(cache) if cache.format == FORMAT && cache.engine == engine => cache,
        _ => fresh(),
    }
}

/// Writes the cache, creating its directory when needed.
///
/// # Errors
/// Fails when the directory cannot be created or the file cannot be
/// written. Callers treat this as a missed speed-up, not a failure.
pub fn save(path: &Path, cache: &StaticCache) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string(cache).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_profiler_model::{Category, Confidence, SourceLocation};

    #[test]
    fn hash_changes_with_contents_and_is_stable() {
        assert_eq!(hash_source("fn a() {}"), hash_source("fn a() {}"));
        assert_ne!(hash_source("fn a() {}"), hash_source("fn b() {}"));
    }

    #[test]
    fn finding_survives_a_round_trip_through_the_cache_form() {
        let finding = Finding {
            category: Category::Allocation,
            detector: "allocation",
            description: String::from("allocates in a loop"),
            location: SourceLocation::at_line(String::from("src/lib.rs"), 7),
            confidence: Confidence::Medium,
            numerical_context: true,
        };
        let back = CachedFinding::from_finding(&finding).into_finding();
        assert_eq!(back, finding);
    }

    #[test]
    fn corrupt_or_foreign_caches_load_empty() {
        let dir = std::env::temp_dir().join(format!("tpt-cache-load-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join(CACHE_FILE);

        std::fs::write(&path, "{ not json").expect("write");
        assert_eq!(load(&path, "k").files.len(), 0);

        let other = StaticCache {
            format: FORMAT,
            engine: String::from("other-engine"),
            files: BTreeMap::from([(
                String::from("a.rs"),
                CachedFile {
                    hash: 1,
                    findings: Vec::new(),
                },
            )]),
        };
        save(&path, &other).expect("save");
        assert_eq!(load(&path, "k").files.len(), 0);
        assert_eq!(load(&path, "other-engine").files.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cached_run_matches_a_fresh_run_and_reuses_unchanged_files() {
        use crate::engine::StaticEngine;

        let root = std::env::temp_dir().join(format!("tpt-cache-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).expect("src");
        let busy = "pub fn names(v: &[u32]) -> Vec<String> {
    let mut out = Vec::new();
    for i in v {
        out.push(i.to_string());
    }
    out
}
";
        std::fs::write(root.join("src").join("a.rs"), busy).expect("a");
        std::fs::write(
            root.join("src").join("b.rs"),
            "pub fn b() {}
",
        )
        .expect("b");
        let cache_path = root.join(".tpt-profiler").join("cache").join(CACHE_FILE);

        let engine = StaticEngine::with_default_analyzers();
        let fresh = engine.analyze_path(&root).expect("fresh");
        assert!(
            !fresh.findings.is_empty(),
            "fixture should produce findings"
        );

        let first = engine
            .analyze_path_cached(&root, &cache_path)
            .expect("first");
        assert_eq!(first.cache_hits, 0);
        assert_eq!(first.findings, fresh.findings);

        let second = engine
            .analyze_path_cached(&root, &cache_path)
            .expect("second");
        assert_eq!(second.cache_hits, 2);
        assert_eq!(second.findings, fresh.findings);

        std::fs::write(
            root.join("src").join("b.rs"),
            "pub fn b() { let x = 1; }
",
        )
        .expect("edit");
        let third = engine
            .analyze_path_cached(&root, &cache_path)
            .expect("third");
        assert_eq!(third.cache_hits, 1, "only the edited file is re-analysed");
        assert_eq!(
            third.findings,
            engine.analyze_path(&root).expect("fresh again").findings
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
