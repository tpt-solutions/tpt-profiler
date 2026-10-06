# tpt-profiler — Design Notes

Derived from the architecture in `spec.txt` (§5–6). `spec.txt` remains the
source of truth; this document records engineering decisions and the rules
that keep the workspace coherent.

## 1. Architecture overview (spec §5)

```text
Repository → Project Discovery → { Static, Runtime, Build } Analysis
           → Evidence Collection → Opportunity Engine → Priority Calculator
           → { Terminal, JSON, CI } reports → Engineering Queue
```

The pipeline separates four concerns that spec §51 requires to stay
distinct: **Measurement** (observed facts), **Evidence** (typed,
source-attributed support for a finding), **Interpretation**
(what the evidence means), and **Recommendation** (what to do). The
opportunity engine converts evidence into opportunities; it never
invents measurements.

## 2. Crate dependency direction

```text
                    tpt-profiler-model        (no_std-capable leaf)
                          ▲
        ┌─────────┬───────┼────────┬──────────────┐
        │         │       │        │              │
     -static  -runtime  -bench  -memory  -build  -opportunity
        │         │       │        │       │        │
        └─────────┴───┬───┴────────┴───────┴────────┘
                      ▼
               tpt-profiler (core, orchestration)
                      ▲
                      │
               tpt-profiler-report   ← depends on model (+ core types)
                      ▲
                      │
               tpt-profiler-cli      → core + report
```

Rules:

* **`tpt-profiler-model` is the leaf.** It depends only on `core`-ecosystem
  essentials (`thiserror`, optionally `serde` with `default-features =
  false`). Everything else may depend on it; it depends on nothing else in
  the workspace. No cycles.
* Analysis crates (`-static`, `-runtime`, `-bench`, `-memory`, `-build`)
  depend on `model` only — never on each other.
* `-opportunity` depends on `model` and consumes evidence produced by the
  analysis crates (evidence types live in `model`).
* `tpt-profiler` (core) is the only crate that may depend on all analysis
  crates: it owns the orchestrator pipeline and the analyzer registry.
* `-report` renders model types; it must not depend on analysis crates.
* `-cli` is the only binary; it wires config, core, and report together.

## 3. `no_std` strategy (spec §73)

* `tpt-profiler-model` is `#![no_std]` + `extern crate alloc` by default;
  a `std` feature is available where OS-specific helpers are genuinely
  needed. `serde` is optional and pulled in with `default-features =
  false` (serde's `std` feature must stay off in this crate).
* CI builds `tpt-profiler-model` for a bare-metal target
  (`thumbv7em-none-eabi`) with `--no-default-features` to guarantee the
  claim holds.
* All other crates are `std`-only: profiling, process handling, and
  reporting need an OS.

## 4. Error taxonomy (spec §74)

`model::error` defines the structured error taxonomy:
`DiscoveryError`, `ConfigurationError`, `ProfilingError`,
`BenchmarkError`, `AnalysisError`, `ToolUnavailable`, `ParseError`,
`SerializationError`, `EnvironmentError`, all foldable into the top-level
`Error`.

**"No opportunity detected" is not an error.** Analyzers return evidence
collections; an empty collection with `Ok` status means "nothing found".
Errors are reserved for actual failures (tool missing, parse/IO errors).
`AnalysisOutcome` makes this explicit at the pipeline boundary.

## 5. Schema versioning (spec §69)

All persisted/emitted JSON is wrapped in a versioned envelope:

```json
{ "schema_version": 1, "generated_by": "tpt-profiler 0.1.0", "data": { … } }
```

* `SCHEMA_VERSION` is a constant in `tpt-profiler-model`; it increments on
  any breaking change to the persisted format.
* Readers must reject or migrate data with an unknown `schema_version`
  (never silently reinterpret).
* Every release publishes the JSON schema and validates it in tests.

## 6. Deterministic opportunity IDs (spec §25)

IDs are stable across runs: `<CATEGORY>-<NNNNN>` where `NNNNN` is the
zero-padded, low 20 bits of a stable FNV-1a hash over
`category | component | description` (normalised, lower-case). FNV-1a is
implemented locally (no external hash crate, stable across platforms and
compiler versions — unlike `DefaultHasher`). Collisions are resolved by
appending `-1`, `-2`, … deterministically in ID assignment order.
Static analysis must therefore emit findings in deterministic order
(sorted by file, then span — spec §75) so assignment is reproducible.

## 7. Scoring and API stability (spec §27)

The priority formula (impact × confidence × generality / effort × risk,
normalised 0–100) is **not** an API contract. It is configurable via
`[scoring]` in `tpt-profiler.toml` and may change in any release.
The public API surface is: the core orchestrator entry points, the model
types (with serde), and the versioned JSON schema. Internal crates
(`-*`) may change without notice; their versions move in lockstep with
the core.

## 8. Logging & tracing

* The `log` facade (not `tracing`) — lightweight, `no_std`-friendly, and
  sufficient for a CLI tool's needs.
* Logs go to stderr or a local file only (`--verbose` raises the level).
  **No network logging, ever** (spec §70).
* Structured run records (JSON) are the durable output; human logs are
  ephemeral.

## 9. Privacy stance (spec §70)

Local-only by design: no network I/O, no telemetry, no source upload.
Enforced technically, not just by policy:

* `deny.toml` bans network-capable crates (`reqwest`, `ureq`, `hyper`,
  `curl`, …); CI fails if one enters the dependency graph.
* No crate in the workspace may enable `tokio` net features.

## 10. Toolchain & MSRV

* **MSRV: 1.85** (first compiler supporting edition 2024), declared via
  `rust-version` in `[workspace.package]` and enforced by a CI job.
* `rust-toolchain.toml` pins the *development* toolchain to `stable`
  with `rustfmt` and `clippy` components; MSRV compatibility is CI's job,
  not the dev toolchain's.

## 11. Dependency policy

Adding a dependency requires:

1. **Licence** — MIT, dual MIT/Apache-2.0, or another approved permissive
   licence. **Apache-2.0-only is rejected** (`deny.toml` encodes this by
   omitting Apache-2.0 from the allow list: dual-licensed crates pass via
   their MIT clause, Apache-only crates fail).
2. **Patent / redistribution review** (spec §71).
3. **Necessity** — mainstream, actively maintained crates only (clap,
   serde, syn, cargo_metadata, thiserror, toml, ignore, …). No
   single-author micro-crates without review.
4. **No network capability.**
5. Record the decision in the PR description.

## 12. Testing strategy (spec §76)

* Unit tests next to the code (detectors, scoring, models, parsers).
* Cross-crate integration tests in `tests/` against fixture workspaces
  (`tests/fixtures/`).
* Golden tests for terminal/JSON/Markdown output.
* Regression corpus: known opportunities must stay detectable.
* The profiler benchmarks itself (spec §72) — it must stay fast and
  allocation-light.
