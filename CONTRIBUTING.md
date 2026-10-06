# Contributing to tpt-profiler

Thank you for your interest in contributing! This document explains how to
get started and the rules that keep the project consistent.

## Licensing

By submitting a contribution, you agree that it is licensed under **both**
the MIT licence and the Apache Licence 2.0, without any additional terms or
conditions. You retain copyright; each contribution is covered by the
standard Apache 2.0 §5 submission terms.

## Getting started

1. Fork / clone the repository (default branch: `master`).
2. Install a Rust toolchain (see `rust-toolchain.toml`; MSRV is 1.85).
3. `cargo build` and `cargo test` should pass out of the box.
4. Install the local check helper if you have [`just`](https://github.com/casey/just):
   `just --list` shows all available checks.

## Before opening a PR

Run the same checks CI runs:

```bash
just check        # fmt + clippy + test
just deny         # cargo deny (licences, bans, advisories)
```

* `cargo fmt --all -- --check` must be clean.
* `cargo clippy --workspace --all-targets -- -D warnings` must be clean.
* New dependencies require review (see the dependency policy in
  [DESIGN.md](DESIGN.md) — Apache-2.0-only dependencies are rejected).

## Project rules (from spec.txt)

* **No source modification.** The profiler observes, measures, analyses,
  recommends, and validates — it never rewrites code (spec §53).
* **Privacy.** No network access, no telemetry, no source upload. Any
  future network feature must be explicit and opt-in (spec §70).
* **No heavy dependencies.** The profiler itself must stay efficient
  (spec §72).
* **Determinism.** Static analysis output must be deterministic and
  ordered (spec §75).
* **Evidence, not guesses.** Findings must include evidence and
  confidence; never recommend optimisation where evidence is insufficient
  (spec §3.1.12, §9).

## Code style

* Edition 2024, workspace lints from the root `Cargo.toml`.
* `unsafe_code = "forbid"` everywhere; no exceptions without a
  dedicated discussion.
* Errors are structured (see `tpt-profiler-model::error`); distinguish
  "no opportunity detected" from "profiling failed".
* Public API items require doc comments; `cargo doc` must build without
  warnings.

## Reporting issues

For bugs, include the command, the target repository shape, and the
output. For false-positive opportunities, include the reason — dismissal
reasons feed back into heuristic tuning (spec §52).
