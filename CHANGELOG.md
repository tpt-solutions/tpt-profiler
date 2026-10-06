# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
