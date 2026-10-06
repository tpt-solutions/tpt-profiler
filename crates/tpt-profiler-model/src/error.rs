//! Structured error taxonomy (spec §74).
//!
//! Every failure carries structured context (paths, tool names, messages)
//! so callers — including the CLI — can render actionable diagnostics
//! without string parsing. The top-level [`Error`] folds all categories.
//!
//! Per spec §74, *"no opportunity detected"* is **not** an error: see
//! [`crate::outcome::AnalysisOutcome`].

use alloc::string::String;

/// Convenience alias for results that fail with the top-level [`Error`].
pub type Result<T> = core::result::Result<T, Error>;

/// Top-level error type folding every error category of spec §74.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Repository/workspace discovery failed.
    #[error(transparent)]
    Discovery(#[from] DiscoveryError),
    /// Configuration loading or validation failed.
    #[error(transparent)]
    Configuration(#[from] ConfigurationError),
    /// Runtime profiling failed.
    #[error(transparent)]
    Profiling(#[from] ProfilingError),
    /// Benchmark discovery or execution failed.
    #[error(transparent)]
    Benchmark(#[from] BenchmarkError),
    /// An analyzer failed mid-run.
    #[error(transparent)]
    Analysis(#[from] AnalysisError),
    /// A required external tool is not installed (spec §68, §74).
    #[error(transparent)]
    ToolUnavailable(#[from] ToolUnavailableError),
    /// A file could not be parsed.
    #[error(transparent)]
    Parse(#[from] ParseError),
    /// Encoding/decoding of profiler data failed.
    #[error(transparent)]
    Serialization(#[from] SerializationError),
    /// The execution environment could not be captured or is unsuitable.
    #[error(transparent)]
    Environment(#[from] EnvironmentError),
}

/// Errors raised while discovering repository/project structure (spec §7).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DiscoveryError {
    /// No `Cargo.toml` was found at or above the given path.
    #[error("no Cargo.toml found at or above `{path}`; not a Rust project")]
    NoManifest {
        /// Path where the search started.
        path: String,
    },
    /// A manifest exists but could not be read.
    #[error("could not read manifest `{path}`: {message}")]
    ManifestReadFailed {
        /// Manifest path.
        path: String,
        /// Underlying message (I/O details are environment-specific).
        message: String,
    },
    /// `cargo metadata` failed or returned unusable data.
    #[error("cargo metadata failed: {message}")]
    MetadataFailed {
        /// Diagnostics from cargo.
        message: String,
    },
    /// The layout is a Rust project but not one the profiler can model.
    #[error("unsupported project layout at `{path}`: {message}")]
    UnsupportedLayout {
        /// Project path.
        path: String,
        /// What is unsupported about it.
        message: String,
    },
}

/// Errors raised while loading or validating `tpt-profiler.toml` (spec §40).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigurationError {
    /// A config file exists but could not be read.
    #[error("could not read config `{path}`: {message}")]
    ReadFailed {
        /// Config path.
        path: String,
        /// Underlying message.
        message: String,
    },
    /// The config file is not valid TOML or does not match the schema.
    #[error("invalid config `{path}`: {message}")]
    ParseFailed {
        /// Config path.
        path: String,
        /// TOML/schema diagnostics.
        message: String,
    },
    /// A value is structurally valid but semantically rejected.
    #[error("invalid config value for `{field}`: {message}")]
    ValidationFailed {
        /// Dotted config key.
        field: String,
        /// Why the value was rejected.
        message: String,
    },
}

/// Errors raised while running target workloads (spec §8.2).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProfilingError {
    /// The workload process could not be spawned.
    #[error("failed to spawn `{command}`: {message}")]
    SpawnFailed {
        /// Command line attempted.
        command: String,
        /// Underlying message.
        message: String,
    },
    /// The workload exceeded its configured timeout.
    #[error("`{command}` timed out after {timeout_ms} ms")]
    TimedOut {
        /// Command line attempted.
        command: String,
        /// Configured timeout in milliseconds.
        timeout_ms: u64,
    },
    /// The run was cancelled by the user.
    #[error("profiling run cancelled")]
    Cancelled,
}

/// Errors raised by benchmark discovery/execution (spec §8.3).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BenchmarkError {
    /// No benchmarks exist for the requested configuration.
    #[error("no benchmarks found (spec §74: this is a result, not a crash)")]
    NoBenchmarksFound,
    /// A benchmark suite failed to build or run.
    #[error("benchmark `{name}` failed: {message}")]
    ExecutionFailed {
        /// Benchmark name.
        name: String,
        /// Captured diagnostics.
        message: String,
    },
    /// Criterion output data is missing where it was expected.
    #[error("criterion data missing at `{path}`")]
    CriterionDataMissing {
        /// Expected path of criterion output.
        path: String,
    },
}

/// Errors raised by analyzers during a run (spec §67).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnalysisError {
    /// A single analyzer failed; the run may continue with partial
    /// results, but the failure is recorded.
    #[error("analyzer `{analyzer}` failed: {message}")]
    AnalyzerFailed {
        /// Registered analyzer name.
        analyzer: String,
        /// Diagnostics.
        message: String,
    },
}

/// A required external tool is not installed or not usable (spec §68).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolUnavailableError {
    /// The named tool was not found or is too old.
    #[error("tool `{tool}` unavailable: {hint}")]
    Tool {
        /// Tool name (e.g. `perf`, `cargo`, `gh`).
        tool: String,
        /// What the user can do about it.
        hint: String,
    },
}

/// Errors raised while parsing target or profiler files.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// The file exists but its contents are not valid for the format.
    #[error("parse error in `{path}` (line {line}): {message}")]
    InvalidSyntax {
        /// File path.
        path: String,
        /// 1-based line number, when known.
        line: u32,
        /// Parser diagnostics.
        message: String,
    },
    /// The format itself is not supported.
    #[error("unsupported format `{format}` for `{path}`")]
    UnsupportedFormat {
        /// Format identifier.
        format: String,
        /// File path.
        path: String,
    },
}

/// Errors raised while encoding/decoding profiler data (spec §69).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SerializationError {
    /// Encoding profiler data failed.
    #[error("serialization failed: {message}")]
    EncodeFailed {
        /// Diagnostics.
        message: String,
    },
    /// Decoding persisted data failed.
    #[error("deserialization failed: {message}")]
    DecodeFailed {
        /// Diagnostics.
        message: String,
    },
    /// The data was written by a newer, incompatible schema version.
    #[error("schema version {found} is newer than the supported version {supported}")]
    SchemaTooNew {
        /// Version found in the data.
        found: u32,
        /// Maximum version this build supports ([`crate::schema::SCHEMA_VERSION`]).
        supported: u32,
    },
    /// The data is too old to migrate.
    #[error("schema version {found} is older than the minimum supported version {minimum}")]
    SchemaTooOld {
        /// Version found in the data.
        found: u32,
        /// Minimum supported version ([`crate::schema::MIN_SUPPORTED_SCHEMA_VERSION`]).
        minimum: u32,
    },
}

/// Errors raised while capturing the execution environment (spec §48–49).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnvironmentError {
    /// Git metadata was required but the path is not a git repository.
    #[error("`{path}` is not a git repository")]
    NotAGitRepository {
        /// Path checked.
        path: String,
    },
    /// An external query (e.g. `git rev-parse`) failed.
    #[error("environment query failed: {message}")]
    QueryFailed {
        /// Diagnostics.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn display_includes_context() {
        let err = DiscoveryError::NoManifest {
            path: "/tmp/empty".to_string(),
        };
        let rendered = alloc::format!("{err}");
        assert!(rendered.contains("/tmp/empty"), "unexpected: {rendered}");

        let folded: Error = err.into();
        assert!(alloc::format!("{folded}").contains("/tmp/empty"));
    }

    #[test]
    fn conversions_fold_into_top_level_error() {
        let err: Error = SerializationError::SchemaTooNew {
            found: 9,
            supported: 1,
        }
        .into();
        assert_eq!(
            err,
            Error::Serialization(SerializationError::SchemaTooNew {
                found: 9,
                supported: 1
            })
        );
    }

    #[test]
    fn errors_are_comparable_and_clonable() {
        let a = ParseError::InvalidSyntax {
            path: "Cargo.toml".to_string(),
            line: 3,
            message: "bad table".to_string(),
        };
        assert_eq!(a.clone(), a);
    }
}
