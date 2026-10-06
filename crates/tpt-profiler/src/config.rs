//! Configuration loading and validation (spec §40).
//!
//! `tpt-profiler.toml` is discovered from the starting directory upward
//! (stopping at the first hit, typically the workspace root). Precedence
//! is **flags > config > defaults**: CLI flags map onto [`Overrides`],
//! which are applied over the loaded-or-default [`Config`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tpt_profiler_model::error::{ConfigurationError, Error};

/// Default config file name, searched from the working directory upward.
pub const CONFIG_FILE_NAME: &str = "tpt-profiler.toml";

/// Top-level configuration (spec §40).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
#[derive(Default)]
pub struct Config {
    /// Project identity.
    pub project: ProjectConfig,
    /// Which profiling modes run (spec §40).
    pub profiling: ProfilingConfig,
    /// Benchmark integration (spec §8.3).
    pub benchmarks: BenchmarksConfig,
    /// CI policy (spec §39).
    pub ci: CiConfig,
    /// Scoring weights (spec §27: the formula is configurable and *not*
    /// an API contract).
    pub scoring: ScoringConfig,
}

/// `[project]` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
#[derive(Default)]
pub struct ProjectConfig {
    /// Overrides the repository name inferred from the directory.
    pub name: Option<String>,
}

/// `[profiling]` section (spec §40).
// The toggles are independent mode switches, not a state machine.
#[allow(
    clippy::struct_excessive_bools,
    reason = "each profiling mode is an independent opt-in/out switch"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProfilingConfig {
    /// Run workload-based runtime profiling (`tpt-profiler runtime`).
    pub runtime: bool,
    /// Run memory/allocation profiling where tooling permits.
    pub memory: bool,
    /// Run CPU profiling where tooling permits (spec §8.5).
    pub cpu: bool,
    /// Look for SIMD opportunities (spec §12).
    pub simd: bool,
    /// Look for parallelism opportunities (spec §13).
    pub parallelism: bool,
}

/// `[benchmarks]` section (spec §40).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BenchmarksConfig {
    /// Discover and execute benchmarks.
    pub enabled: bool,
}

/// `[ci]` section (spec §39, §40).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CiConfig {
    /// CI checks participate in the run.
    pub enabled: bool,
    /// Fail the run on a detected performance regression.
    pub fail_on_regression: bool,
    /// Runtime regression threshold as a fraction (0.10 = fail above 10%).
    pub runtime_threshold: f64,
    /// Binary-size growth threshold as a fraction (0.05 = warn above 5%).
    pub binary_threshold: f64,
}

/// `[scoring]` section: weights for the priority formula (spec §27).
///
/// The formula shape itself is deliberately not exposed here (see
/// `DESIGN.md` §7); these multipliers only tune how strongly each factor
/// counts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ScoringConfig {
    /// Weight of the impact factor (spec §28).
    pub impact_weight: f64,
    /// Weight of the confidence factor (spec §29).
    pub confidence_weight: f64,
    /// Weight of the generality factor (spec §32).
    pub generality_weight: f64,
    /// Weight of the effort divisor (spec §30).
    pub effort_weight: f64,
    /// Weight of the risk divisor (spec §31).
    pub risk_weight: f64,
}

impl Default for ProfilingConfig {
    fn default() -> Self {
        Self {
            runtime: true,
            memory: true,
            cpu: true,
            simd: true,
            parallelism: true,
        }
    }
}

impl Default for BenchmarksConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

impl Default for CiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            fail_on_regression: true,
            runtime_threshold: 0.10,
            binary_threshold: 0.05,
        }
    }
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            impact_weight: 1.0,
            confidence_weight: 1.0,
            generality_weight: 1.0,
            effort_weight: 1.0,
            risk_weight: 1.0,
        }
    }
}

/// CLI flag overrides; applied over the config (flags > config > defaults,
/// spec §40 precedence). `None` means "flag not given, keep config value".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Overrides {
    /// `--profiling-runtime/--no-runtime` style flags.
    pub profiling_runtime: Option<bool>,
    /// Memory profiling flag.
    pub profiling_memory: Option<bool>,
    /// CPU profiling flag.
    pub profiling_cpu: Option<bool>,
    /// SIMD detection flag.
    pub profiling_simd: Option<bool>,
    /// Parallelism detection flag.
    pub profiling_parallelism: Option<bool>,
    /// Benchmark execution flag.
    pub benchmarks_enabled: Option<bool>,
    /// CI flag.
    pub ci_enabled: Option<bool>,
}

impl Config {
    /// Parses and validates a config from TOML text.
    pub fn from_toml(path: &Path, text: &str) -> Result<Self, Error> {
        let config: Config = toml::from_str(text).map_err(|error| {
            Error::Configuration(ConfigurationError::ParseFailed {
                path: path.display().to_string(),
                message: error.to_string(),
            })
        })?;
        config.validate().map_err(|message| {
            Error::Configuration(ConfigurationError::ValidationFailed {
                field: message_field(&message),
                message,
            })
        })?;
        Ok(config)
    }

    /// Loads the config file at `path`, or returns defaults when the file
    /// does not exist. An existing but invalid file is an error.
    pub fn load(path: &Path) -> Result<Self, Error> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path).map_err(|error| {
            Error::Configuration(ConfigurationError::ReadFailed {
                path: path.display().to_string(),
                message: error.to_string(),
            })
        })?;
        Self::from_toml(path, &text)
    }

    /// Searches `start` and its ancestors for `tpt-profiler.toml`.
    #[must_use]
    pub fn discover(start: &Path) -> Option<PathBuf> {
        let mut current = Some(start);
        while let Some(dir) = current {
            let candidate = dir.join(CONFIG_FILE_NAME);
            if candidate.is_file() {
                return Some(candidate);
            }
            current = dir.parent();
        }
        None
    }

    /// Loads the config discovered from `start`; defaults when none is
    /// found. Returns the config and the file it came from (if any).
    pub fn load_discovered(start: &Path) -> Result<(Self, Option<PathBuf>), Error> {
        match Self::discover(start) {
            Some(path) => Ok((Self::load(&path)?, Some(path))),
            None => Ok((Self::default(), None)),
        }
    }

    /// Applies CLI overrides over the loaded config (flags > config).
    pub fn apply_overrides(&mut self, overrides: &Overrides) {
        if let Some(value) = overrides.profiling_runtime {
            self.profiling.runtime = value;
        }
        if let Some(value) = overrides.profiling_memory {
            self.profiling.memory = value;
        }
        if let Some(value) = overrides.profiling_cpu {
            self.profiling.cpu = value;
        }
        if let Some(value) = overrides.profiling_simd {
            self.profiling.simd = value;
        }
        if let Some(value) = overrides.profiling_parallelism {
            self.profiling.parallelism = value;
        }
        if let Some(value) = overrides.benchmarks_enabled {
            self.benchmarks.enabled = value;
        }
        if let Some(value) = overrides.ci_enabled {
            self.ci.enabled = value;
        }
    }

    /// Rejects semantically invalid values with precise messages
    /// (spec §3.1.14, §40: good error messages).
    pub fn validate(&self) -> Result<(), String> {
        for (name, threshold) in [
            ("ci.runtime_threshold", self.ci.runtime_threshold),
            ("ci.binary_threshold", self.ci.binary_threshold),
        ] {
            if !(0.0..=1.0).contains(&threshold) || threshold.is_nan() {
                return Err(format!(
                    "{name} must be a fraction in 0.0..=1.0, got {threshold}"
                ));
            }
        }
        for (name, weight) in [
            ("scoring.impact_weight", self.scoring.impact_weight),
            ("scoring.confidence_weight", self.scoring.confidence_weight),
            ("scoring.generality_weight", self.scoring.generality_weight),
            ("scoring.effort_weight", self.scoring.effort_weight),
            ("scoring.risk_weight", self.scoring.risk_weight),
        ] {
            if !weight.is_finite() || weight <= 0.0 {
                return Err(format!("{name} must be finite and > 0, got {weight}"));
            }
        }
        Ok(())
    }
}

/// Extracts a leading dotted field name from a validation message (the
/// messages above all start with `<field> must …`).
fn message_field(message: &str) -> String {
    message
        .split_whitespace()
        .next()
        .unwrap_or("unknown")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec_39_and_40_examples() {
        let config = Config::default();
        assert!(config.profiling.runtime);
        assert!(config.profiling.simd);
        assert!(config.benchmarks.enabled);
        assert!(config.ci.fail_on_regression);
        assert!((config.ci.runtime_threshold - 0.10).abs() < f64::EPSILON);
        assert!((config.ci.binary_threshold - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn parses_spec_40_example() {
        let text = r#"
            [project]
            name = "tpt-cadence"

            [profiling]
            runtime = true
            memory = true
            cpu = true
            simd = true

            [benchmarks]
            enabled = true

            [ci]
            enabled = true
            fail_on_regression = true
        "#;
        let config = Config::from_toml(Path::new("tpt-profiler.toml"), text).expect("parse");
        assert_eq!(config.project.name.as_deref(), Some("tpt-cadence"));
        assert!(config.profiling.runtime);
    }

    #[test]
    fn rejects_unknown_fields_with_the_field_name() {
        let text = "[profiling]\nrunntime = true\n"; // typo
        let error = Config::from_toml(Path::new("c.toml"), text).expect_err("must fail");
        let rendered = format!("{error}");
        assert!(rendered.contains("runntime"), "unexpected: {rendered}");
    }

    #[test]
    fn rejects_out_of_range_thresholds_and_weights() {
        let bad_threshold = "[ci]\nruntime_threshold = 1.5\n";
        assert!(Config::from_toml(Path::new("c.toml"), bad_threshold).is_err());

        let bad_weight = "[scoring]\nimpact_weight = 0.0\n";
        assert!(Config::from_toml(Path::new("c.toml"), bad_weight).is_err());
    }

    #[test]
    fn overrides_win_over_config() {
        let text = "[profiling]\nruntime = true\nsimd = false\n";
        let mut config = Config::from_toml(Path::new("c.toml"), text).expect("parse");
        config.apply_overrides(&Overrides {
            profiling_runtime: Some(false),
            ..Overrides::default()
        });
        assert!(!config.profiling.runtime);
        assert!(
            !config.profiling.simd,
            "un-overridden config value survives"
        );
    }

    #[test]
    fn missing_file_yields_defaults() {
        let config = Config::load(Path::new("definitely/missing/tpt-profiler.toml"))
            .expect("missing file is not an error");
        assert_eq!(config, Config::default());
    }

    #[test]
    fn discovery_walks_upward() {
        let temp = std::env::temp_dir().join("tpt-config-test-upward");
        let nested = temp.join("a/b/c");
        std::fs::create_dir_all(&nested).expect("mkdir");
        let config_path = temp.join(CONFIG_FILE_NAME);
        std::fs::write(&config_path, "[project]\nname = \"root\"\n").expect("write");

        let found = Config::discover(&nested).expect("found upward");
        assert_eq!(found, config_path);
        let (config, source) = Config::load_discovered(&nested).expect("load discovered");
        assert_eq!(config.project.name.as_deref(), Some("root"));
        assert_eq!(source.as_deref(), Some(config_path.as_path()));

        std::fs::remove_dir_all(&temp).expect("cleanup");
    }

    #[test]
    fn toml_round_trip_preserves_values() {
        let text = "[profiling]\nruntime = false\n\n[ci]\nruntime_threshold = 0.2\n";
        let config = Config::from_toml(Path::new("c.toml"), text).expect("parse");
        let serialised = toml::to_string(&config).expect("encode");
        let reparsed: Config = toml::from_str(&serialised).expect("re-parse");
        assert_eq!(reparsed, config);
    }
}
