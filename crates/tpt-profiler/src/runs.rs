//! Named stored runs (spec §42): a full analysis saved under a name so
//! `report <name>` can print it again later, without the source present.
//!
//! Layout:
//!
//! ```text
//! .tpt-profiler/
//! └── runs/
//!     └── <name>.json   (SchemaEnvelope<ProfileRun>)
//! ```

use std::path::{Path, PathBuf};

use tpt_profiler_model::error::{Error, SerializationError};
use tpt_profiler_model::{ProfileRun, SchemaEnvelope};

use crate::baseline::STATE_DIR;

/// Subdirectory holding named runs.
pub const RUNS_DIR: &str = "runs";

/// Path of the runs directory under a workspace root.
#[must_use]
pub fn runs_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join(STATE_DIR).join(RUNS_DIR)
}

/// Whether `name` can be used as a run name: non-empty, and only letters,
/// digits, `-` and `_`, so it can never escape the runs directory.
#[must_use]
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
}

fn invalid_name(name: &str) -> Error {
    Error::Serialization(SerializationError::EncodeFailed {
        message: format!("run name `{name}` may only contain letters, digits, `-` and `_`"),
    })
}

/// Saves a run under `name`, replacing any earlier run with that name.
///
/// # Errors
/// Fails on an invalid name, filesystem errors, or when the run cannot be
/// serialised (spec §74).
pub fn save_run(workspace_root: &Path, name: &str, run: &ProfileRun) -> Result<PathBuf, Error> {
    if !is_valid_name(name) {
        return Err(invalid_name(name));
    }
    let dir = runs_dir(workspace_root);
    std::fs::create_dir_all(&dir).map_err(|error| {
        Error::Serialization(SerializationError::EncodeFailed {
            message: format!("could not create {}: {error}", dir.display()),
        })
    })?;
    let path = dir.join(format!("{name}.json"));
    let envelope = SchemaEnvelope::new(
        format!("tpt-profiler {}", tpt_profiler_model::version()),
        run,
    );
    let json = serde_json::to_string_pretty(&envelope).map_err(|error| {
        Error::Serialization(SerializationError::EncodeFailed {
            message: error.to_string(),
        })
    })?;
    std::fs::write(&path, json).map_err(|error| {
        Error::Serialization(SerializationError::EncodeFailed {
            message: format!("could not write {}: {error}", path.display()),
        })
    })?;
    Ok(path)
}

/// Loads a named run; `None` when it does not exist.
///
/// # Errors
/// Fails on an invalid name, unreadable files, corrupt JSON, or a schema
/// version this build cannot interpret.
pub fn load_run(workspace_root: &Path, name: &str) -> Result<Option<ProfileRun>, Error> {
    if !is_valid_name(name) {
        return Err(invalid_name(name));
    }
    let path = runs_dir(workspace_root).join(format!("{name}.json"));
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|error| {
        Error::Serialization(SerializationError::DecodeFailed {
            message: format!("could not read {}: {error}", path.display()),
        })
    })?;
    let envelope: SchemaEnvelope<ProfileRun> = serde_json::from_str(&text).map_err(|error| {
        Error::Serialization(SerializationError::DecodeFailed {
            message: format!("{} is corrupt: {error}", path.display()),
        })
    })?;
    envelope.validate().map_err(Error::Serialization)?;
    Ok(Some(envelope.data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_profiler_model::Environment;

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("tpt-runs-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }

    #[test]
    fn names_stay_inside_the_runs_directory() {
        assert!(is_valid_name("release-2026_10"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("../escape"));
        assert!(!is_valid_name("a/b"));
        assert!(!is_valid_name("a b"));
    }

    #[test]
    fn missing_run_is_a_state_and_saved_run_round_trips() {
        let root = temp_root("round-trip");
        assert!(load_run(&root, "nightly").expect("load").is_none());

        let run = ProfileRun::new("run-1", 1_760_000_000, Environment::default());
        save_run(&root, "nightly", &run).expect("save");
        let loaded = load_run(&root, "nightly").expect("load").expect("present");
        assert_eq!(loaded, run);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn invalid_names_are_refused_before_touching_disk() {
        let root = temp_root("invalid");
        let run = ProfileRun::new("run-1", 1_760_000_000, Environment::default());
        assert!(save_run(&root, "../escape", &run).is_err());
        assert!(load_run(&root, "../escape").is_err());
        assert!(!runs_dir(&root).exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
