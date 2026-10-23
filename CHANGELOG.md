# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

* MVP analysis (spec §62, Phase 1): workspace discovery, static analysis
  (allocation, clone, data-structure, loop, SIMD and parallelism candidates,
  unsafe-region tagging, numerical-context recognition, suppression comments),
  dependency-graph analysis (duplicates, feature surface, large build
  dependencies), workload and Criterion measurement with a statistical engine,
  an opportunity engine with impact/confidence/effort/risk/generality scoring,
  and baseline comparison that is noise-aware.
* `tpt-profiler` CLI: `scan`, `static`, `runtime`, `bench`, `full`,
  `baseline`, `compare`, `report`, `opportunities --strategy --top`,
  `explain`, `completions`, and `man`. Global `--format terminal|json|markdown`,
  `--output`, `--config`, `--path`, `--quiet`, `--verbose`. Exit code `0` for
  any result (including "no opportunities"), `1` for failures.
* Core library `tpt-profiler`: `pipeline::analyse` and `analyse_with` (custom
  static engines via `StaticEngine::register`), a runnable example, and a
  `std` feature forwarded to the model crate.
* Versioned JSON output (schema version 1) and Markdown output.
* Documentation: CLI, configuration, and JSON schema references under `docs/`.

* Project infrastructure: dual MIT / Apache-2.0 licensing, `cargo deny`
  configuration (rejecting Apache-2.0-only dependencies), workspace lints,
  MSRV 1.85, GitHub Actions CI (fmt, clippy, tests on Windows/Linux/macOS,
  MSRV build, `no_std` build check, docs build, cargo-deny), and
  Dependabot configuration.
* Cargo workspace skeleton with ten crates (spec §61):
  `tpt-profiler` (core), `tpt-profiler-cli`, `tpt-profiler-model`,
  `tpt-profiler-static`, `tpt-profiler-runtime`, `tpt-profiler-bench`,
  `tpt-profiler-memory`, `tpt-profiler-build`, `tpt-profiler-opportunity`,
  `tpt-profiler-report`.
* `tpt-profiler-model`: structured error taxonomy (spec §74), schema
  versioning constants (spec §69), deterministic opportunity ID generation
  (spec §25), stable FNV-1a hashing utility.
* Documentation: README, DESIGN, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT,
  this changelog.

[Unreleased]: https://github.com/tpt-solutions/tpt-profiler/compare/v0.0.0...HEAD
