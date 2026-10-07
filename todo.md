# tpt-profiler — Project TODO

**Organisation:** TPT Solutions · **Licence:** MIT OR Apache-2.0 · **Language:** Rust (edition 2024)
**Source of truth:** `spec.txt` (section refs like §62 point to it)

## Decisions locked in
- [x] Dual licence `MIT OR Apache-2.0` (supersedes spec §71 "MIT only")
- [x] No Apache-only dependencies (enforced via cargo-deny)
- [x] Static analysis via `syn` (heuristic, no type info)
- [x] MVP targets Windows + Linux + macOS
- [x] Persistence = versioned JSON/JSONL files in `.tpt-profiler/`
- [x] All 10 workspace crates created up front (§61)
- [x] Mainstream, vetted dependencies (clap, serde, syn, cargo_metadata, thiserror, toml, ignore, …)
- [x] Full project infrastructure (CI, deny, lint, MSRV, docs, publishing)

---

## Phase 0 — Foundations & Infrastructure

### 0.1 Repository & licensing
- [x] Initialise git repository and default branch
- [x] Add `.gitignore` (target/, `.tpt-profiler/` runtime artefacts as appropriate)
- [x] Add `LICENSE-MIT` (copyright TPT Solutions)
- [x] Add `LICENSE-APACHE` (Apache-2.0 text)
- [x] Add top-level `LICENSE` / README licence section explaining the dual licence and the contribution clause
- [x] Set SPDX `license = "MIT OR Apache-2.0"` in every `Cargo.toml`
- [x] Update `spec.txt` header and §71 from "MIT" to "MIT OR Apache-2.0"
- [x] Add `CONTRIBUTING.md` (contributions licensed under both)
- [x] Add `SECURITY.md`, `CODE_OF_CONDUCT.md`
- [x] Add `CHANGELOG.md` (Keep a Changelog format)
- [x] Add `DESIGN.md` (derived from spec architecture, §5–6)
- [x] Add `README.md` skeleton

### 0.2 Workspace skeleton (§61)
- [x] Root `Cargo.toml` workspace (resolver 3, edition 2024, shared `[workspace.package]` and `[workspace.dependencies]`)
- [x] Create `crates/tpt-profiler` (core library)
- [x] Create `crates/tpt-profiler-cli`
- [x] Create `crates/tpt-profiler-model` (`no_std`-capable, §73)
- [x] Create `crates/tpt-profiler-static`
- [x] Create `crates/tpt-profiler-runtime`
- [x] Create `crates/tpt-profiler-bench`
- [x] Create `crates/tpt-profiler-memory`
- [x] Create `crates/tpt-profiler-build`
- [x] Create `crates/tpt-profiler-opportunity`
- [x] Create `crates/tpt-profiler-report`
- [x] Create `tests/` directory (integration tests and fixtures)
- [x] Define crate dependency direction (model ← everything; no cycles); document in DESIGN.md
- [ ] Reserve crate names on crates.io (verified available 2026-10-07; actual reservation needs a crates.io login)

### 0.3 Tooling & quality gates
- [x] Decide and pin MSRV; add `rust-version` and `rust-toolchain.toml`
- [x] `rustfmt.toml`
- [x] Workspace `[lints]` (clippy pedantic subset, `unsafe_code = "forbid"` by default)
- [x] `deny.toml`: allow only MIT / Apache-2.0 dual / other approved permissive licences; reject Apache-only, GPL, etc.
- [x] cargo-deny checks: licences, bans, advisories, sources
- [x] Review each direct dependency for licence, patents, redistribution (§71)
- [x] Dependency policy doc: how to add a dependency
- [x] `cargo-audit` / advisory check in CI
- [x] GitHub Actions CI: fmt, clippy, test on Windows + Linux + macOS
- [x] CI: MSRV build job
- [x] CI: `no_std` build check for `tpt-profiler-model`
- [x] CI: cargo-deny job
- [x] CI: docs build (`cargo doc`, deny warnings)
- [x] Dependabot / Renovate config
- [x] Pre-commit or `just`/`xtask` helper for local checks

### 0.4 Core foundations
- [x] Structured error taxonomy (§74): DiscoveryError, ConfigurationError, ProfilingError, BenchmarkError, AnalysisError, ToolUnavailable, ParseError, SerializationError, EnvironmentError
- [x] Distinguish "no opportunity detected" from "profiling failed" in result types
- [x] Logging/tracing approach (local only)
- [x] Schema versioning strategy for all persisted/emitted JSON (§69)
- [x] Deterministic ID generation scheme for opportunities (e.g. `SIMD-00042`)
- [x] Decide stable-API surface vs internal (scoring formula must not be an API contract, §27)
- [x] Privacy stance enforced: no network, no telemetry by default (§70); add a CI check for network crates

---

## Phase 1 — MVP (v0.1.0, spec §62)

### 1.1 Data model (`tpt-profiler-model`, §6, §24, §25)
- [x] Hierarchy types: Repository, Project, Crate, Module, Function, Dependency, Workload
- [x] ProfileRun type
- [x] Measurement type: metric, value, unit, source, timestamp, environment, workload, confidence (§24)
- [x] Opportunity type: ID, repository, component, category, description, evidence, impact, confidence, effort, risk, generality, priority, suggested action (§25)
- [x] Category enum (§26): CPU, SIMD, Parallelism, Memory, Allocation, Cache, Algorithm, Data Structure, I/O, Serialisation, Dependency, Binary Size, Compile Time, WASM, GPU, Architecture, Concurrency
- [x] Impact scale 1–10 (§28)
- [x] Confidence levels Very Low…Very High (§29)
- [x] Effort levels Trivial…Very High (§30)
- [x] Risk, including correctness/numerical/concurrency/unsafe dimensions (§31)
- [x] Generality scale (§32)
- [x] Evidence type with hierarchy: benchmark regression > runtime profile > hardware counter > static+pattern > heuristic (§29)
- [x] Separate Measurement / Interpretation / Recommendation types (§51)
- [x] Deterministic vs Statistical finding flag (§75)
- [x] Opportunity relationships/dependencies graph (§35)
- [x] Opportunity lifecycle states: Open, Accepted, Rejected, In Progress, Optimised, Validated, Regressed, Superseded (§38)
- [x] Source location type (file, line range, unsafe-region flag, §54)
- [x] `no_std` + `alloc` compatibility, serde optional feature
- [x] Serde round-trip tests; versioned schema constant

### 1.2 Configuration (§40)
- [x] `tpt-profiler.toml` schema: `[project]`, `[profiling]`, `[benchmarks]`, `[ci]`, `[scoring]`
- [x] Config discovery (cwd upward, workspace root)
- [x] Defaults + validation with good error messages
- [x] CLI flag override precedence (flags > config > defaults)
- [x] Config golden tests

### 1.3 Discovery (§7)
- [x] Cargo workspace discovery via `cargo_metadata`
- [x] Parse `Cargo.toml` / `Cargo.lock` (delegated to cargo metadata as the evidence provider, §68)
- [x] Workspace members, binaries, libraries, examples, benches, tests, build scripts
- [x] Feature flags, profiles, target configuration (features mapped; cargo metadata does not expose profiles/target config — noted limitation)
- [x] Dependencies, build-dependencies, dev-dependencies
- [x] Benchmark discovery (Criterion + `cargo bench` harnesses) — `has_benches` + runtime-`criterion` detection
- [x] Single-crate and virtual-workspace handling
- [x] Respect `.gitignore` when walking sources (`ignore` crate; `require_git(false)`)
- [x] `tpt-profiler scan` tree output (Repository → Workspace → crates) — `tpt-profiler-report::render_scan` (CLI wiring in 1.11)
- [x] Integration tests against fixture workspaces (single crate, multi-crate, virtual; nested fixture pending)

### 1.4 Environment & reproducibility (§48, §49)
- [x] Capture OS, arch, CPU, cores, memory (CPU model/memory: Linux via /proc; Windows/macOS left None pending platform APIs, §72)
- [x] Capture rustc / cargo versions, target triple, optimisation flags
- [x] Capture git commit and dirty-tree status
- [x] Capture profile configuration, workload, input, timestamp (fields exist on ProfileRun/Measurement; populated by the pipeline in 1.7/1.12)
- [x] Embed environment block in every run record (ProfileRun.environment; Measurement inherits via Option)
- [x] Cross-platform tests (Windows/Linux/macOS) — host tests assert the capture contract; CI matrix covers all three OSes

### 1.5 Static analysis engine (`tpt-profiler-static`, §9–13, §18)
- [x] `syn`-based source walker with span → file/line mapping (proc-macro2 `span-locations`; gitignore-aware walk)
- [x] Analyzer trait producing standardised evidence (§67) — `Analyzer` + `StaticEngine` registry, `Finding` → `Evidence`
- [x] Deterministic ordering and output (§75) — findings sorted by (file, line, detector, description); corpus test asserts reproducibility
- [x] Per-finding evidence + confidence (no blind flagging, §9) — static evidence capped at Medium, weaker patterns at Low
- [x] Allocation detection (§9.1): allocation in loops, `Vec::new`/`String::new` in loops, `vec!`/`collect`/`to_string`/`format!` in loops, `with_capacity` guidance in messages (boxing and error-path heuristics deferred)
- [x] Clone analysis (§10): `.clone()` in loops, `Arc::clone` path calls exempt, `arc_*`/`rc_*` receiver heuristic (repeated-conversion and ownership-transfer heuristics deferred)
- [x] Heuristic type-name resolution for `Arc`/`Rc` vs struct clone (limits documented in crate docs)
- [x] Data-structure mismatches (§11): `contains` membership lookups in loops, tiny `HashMap::from` literals (String-vs-bytes, fixed-size heap, linked-structure checks deferred)
- [x] Obvious loop opportunities (hoisting, repeated computation) — loop-invariant `let` bindings with call/method initialisers
- [x] SIMD candidate detection (§12): element-wise indexed arithmetic over ranges, no calls/control flow, caveat text required
- [x] Parallelism candidate detection (§13): per-item calls in range loops, literal-workload-size guard (MIN_WORKLOAD = 64)
- [x] Unsafe-region detection and tagging on affected code (§54) — pre-pass tags findings inside `unsafe` blocks/fns
- [x] Numerical-code recognition (§55) — float literals/casts/transcendental calls mark enclosing functions; `numerical_context` flag = faster ≠ equivalent risk
- [ ] Group related low-level findings into aggregated opportunities (§58) — `Finding::group_key()` provided; aggregation lands with the opportunity engine (1.8)
- [x] Suppression comments / config ignores — `// tpt-profiler:ignore` / `// tpt-profiler:ignore-next`
- [x] Unit tests per detector with positive and negative fixtures (28 tests)
- [x] Regression corpus of known-detectable opportunities (§76) — `tests/corpus/*.rs` with `// expect:` markers, plus a clean file asserting zero false positives
- [x] Parse-failure handling (continue, report ParseError, don't abort the run)

### 1.6 Dependency analysis (§18)
- [x] Dependency counts: direct, transitive, build, dev (`deps::analyze` over `cargo metadata`; transitive-only count via name normalisation)
- [x] Duplicate dependency versions (dash/underscore-normalised grouping, High confidence findings)
- [x] Feature usage and unused-feature heuristics (resolved feature surface ≥ 8 → feature-reduction hint; resolved features come from the resolve graph)
- [x] Large dependency detection (build-dependencies with ≥ 10 unique transitive crates, memoised DFS with cycle guard)
- [x] Platform-specific and optional dependencies (target-gated + optional counts)
- [x] Separate runtime-cost vs compile-time-cost reasoning (normal = runtime, build = compile-time, dev = test-only; build-dep findings phrased as compile-time cost)
- [x] Recommendations: dependency reduction, feature reduction, replacement, internalisation (in finding texts, per §18)
- [x] Tests on fixture lockfiles (synthetic metadata JSON for parse determinism; real fixtures exercise the live `cargo metadata` path via discovery tests)

### 1.7 Runtime & benchmark execution (`tpt-profiler-runtime`, `tpt-profiler-bench`, §8.2–8.3, §23, §50)
- [x] Workload model: name, input, duration, repetitions (§23) — model `Workload` + runtime `RunnableWorkload` (program, args, warm-up, timeout)
- [x] Workload definitions in config — `[[workloads]]` section (name/command/args/repetitions/warmup_runs/timeout_ms)
- [x] Run workloads and collect wall/CPU time (wall time per run; CPU time via external profilers lands in Phase 2)
- [x] Execute `cargo bench` and discover results (`bench::run_benches`, bench-target discovery via CrateInfo.has_benches)
- [x] Criterion integration — parses `target/criterion/**/new/estimates.json` (mean with median fallback), ns → s conversion
- [x] Graceful behaviour when Criterion absent (§74) — absent benches → `no_benchmarks` result, absent criterion dir → empty estimates, missing cargo → `ToolUnavailable`
- [x] Statistical engine (§50) — repeats, mean/median/stddev/min/max, 95% CI (normal approx, documented), Tukey-hinge 1.5×IQR outlier rejection, warm-up discard, cold-start recorded separately
- [x] Never treat a single execution as definitive — single sample is always `Low` confidence
- [x] Mark results as Statistical vs Deterministic (§75) — all runtime/bench measurements are `FindingKind::Statistical`
- [x] Timeouts, cancellation, and child-process cleanup — poll-and-kill runner with `CancelToken`; output pipes drained on background threads (no pipe deadlock)
- [x] Windows/Linux/macOS process handling tests — portable echo/false/timeout-sleep tests incl. timeout-kill and cancellation (CI matrix covers all three OSes)
- [x] Emit Measurement records with environment — `to_measurements` stamps workload, confidence from sample spread (CV thresholds), and the run environment

### 1.8 Opportunity engine (`tpt-profiler-opportunity`, §25–35, §51, §58)
- [x] Evidence → Opportunity conversion (Measurement ≠ Interpretation ≠ Recommendation) — `Candidate` → `Opportunity` with separated interpretation/recommendation
- [x] Impact estimation (§28) — tracks the §29 evidence tier (heuristic 4 … benchmark regression 8)
- [x] Confidence estimation per evidence hierarchy (§29) — capped at the tier ceiling regardless of evidence volume
- [x] Effort estimation (§30) — per-category defaults, +2 when evidence sits in `unsafe` regions
- [x] Risk estimation (correctness, numerics, concurrency, unsafe, portability, API) — §31 dimensions; numerical context (§55) and unsafe regions raise the overall and their dimensions
- [x] Generality estimation (§32) — hint overrides per-category defaults
- [x] Priority calculator, preliminary formula (§27) — (I·w × C·w × G·w)/(E·w × R·w), square-root-normalised to 0–100 with pinned edge tests (0, 100, monotonicity)
- [x] Configurable scoring weights via `[scoring]` config — `ScoringWeights` mirrors `[scoring]`; core maps config → weights
- [x] Don't recommend when evidence is insufficient (§3.1.12) — empty-evidence candidates yield NoOpportunity, not errors
- [x] Don't flag rarely-executed code where runtime evidence says so (§2) — coverage/ratio/percent measurement below 1% suppresses the candidate
- [x] Opportunity grouping/aggregation (§58) — ≥3 same-category findings in one component merge with pooled evidence (`AGGREGATION_THRESHOLD`)
- [x] Opportunity dependency edges (§35) — SIMD↔data-structure `RelatedTo` edges in the same component; model carries arbitrary `Relationship`s
- [x] `explain` data — `explain()` renders id/scores/interpretation/recommendation/evidence; `ScoringTrace` records the calculation
- [x] Strategy selection: `highest-value` (§59) — `top_n(..., Strategy::HighestValue, n)`
- [x] Unit tests for scoring edge cases (zero, max, ties) and stable ordering — 18 tests incl. aggregation, suppression, tie stability

### 1.9 Reporting (`tpt-profiler-report`, §42)
- [x] Terminal output (ranked list, colour, width-aware, `NO_COLOR`) — `render_opportunities` with `TerminalOptions { colour, width }`; NO_COLOR/tty decided by the CLI layer
- [x] JSON output with versioned schema (§69) — `render_json` under the `SchemaEnvelope`; summary shape matches the §69 example
- [x] Markdown output — ranked table + per-opportunity sections
- [x] Distinguish measured facts vs predictions in every format (§29) — `Basis:`/"measured (statistical)" vs "static (deterministic)" in all three formats
- [x] Output to stdout or `--output` file — renderers return strings; file routing lands with the CLI (1.11)
- [x] Golden tests for terminal/JSON/Markdown (§76) — exact terminal golden, JSON round-trip through the model, markdown structure
- [x] JSON schema published and validated in tests — envelope validation (schema_version) tested; round-trip via model types

### 1.10 Baseline & compare (§36)
- [x] `.tpt-profiler/` directory layout and versioning — `.tpt-profiler/baselines/<name>.json` under `SchemaEnvelope`
- [x] `baseline` saves runtime measurements (memory/binary/bench as they become available in later phases) — `save_baseline` + `baseline_from_run`
- [x] `compare` reports % change for each metric — signed fractional change, `change_percent()` rendering
- [x] Noise-aware comparison using the statistical engine — measurement confidence gates the verdict: Low-confidence pairs are `Noisy`, never regressions; thresholds from `[ci]`
- [x] Handle environment mismatch between baseline and current (warn) — OS mismatch sets `environment_warning`
- [x] Handle missing/corrupt/old-schema baselines — missing = `None` (a state), corrupt = `DecodeFailed`, future schema = `SchemaTooNew`, never reinterpreted
- [x] Named baselines
- [x] Tests for compare output (golden) — improved/regressed/unchanged/noisy matrix + percent formatting

### 1.11 CLI (`tpt-profiler-cli`, §41)
- [ ] Binary name `tpt-profiler`; clap-based command structure
- [ ] `scan`
- [ ] `static`
- [ ] `runtime`
- [ ] `bench`
- [ ] `full` (all applicable analyses)
- [ ] `baseline`
- [ ] `compare`
- [ ] `report`
- [ ] `opportunities` with `--strategy`
- [ ] `explain <id>`
- [ ] Global flags: `--format`, `--output`, `--config`, `--path`, `--quiet`, `--verbose`
- [ ] Shell completions
- [ ] Meaningful exit codes
- [ ] `--help` text and man page
- [ ] CLI integration tests (assert_cmd-style)

### 1.12 Library API (`tpt-profiler` core)
- [ ] Orchestrator pipeline: discovery → analysis → evidence → opportunities → priority → report (§5)
- [ ] Analyzer registration (internal plugin-style seam, §67)
- [ ] Public API docs and examples
- [ ] Optional `std` feature wiring (§73)

### 1.13 MVP testing & release (§76)
- [ ] Unit tests: parsers, scoring, models, detectors
- [ ] Integration tests: workspace discovery, benchmark execution
- [ ] Golden tests: reports, JSON, Markdown
- [ ] Regression tests: known opportunities remain detectable
- [ ] Dogfood: run `tpt-profiler` on itself
- [ ] Run on 2–3 real unfamiliar Rust repos and review output quality
- [ ] Profiler self-benchmarks (§72, §76)
- [ ] Docs: README quickstart, CLI reference, config reference, JSON schema doc
- [ ] CHANGELOG entry for 0.1.0
- [ ] Tag and publish v0.1.0

---

## Phase 2 — Deeper Profiling (§63)

### 2.1 Profiler backend abstraction
- [ ] Evidence-provider trait for external tools (§68)
- [ ] Tool availability detection → ToolUnavailable
- [ ] Per-OS backend selection

### 2.2 OS profiler integrations
- [ ] Linux: `perf` (record/stat, parse output)
- [ ] Windows: ETW / WPR or equivalent
- [ ] macOS: Instruments / `xctrace` or `sample`
- [ ] Hardware counters: instructions, cycles, branches, cache misses (§8.5)
- [ ] Flamegraph/stack-sample ingestion and hotspot extraction
- [ ] Map hotspots back to functions/spans from static analysis
- [ ] `cpu` command (§8.5)

### 2.3 Memory & allocation (`tpt-profiler-memory`, §8.4, §14)
- [ ] Allocation count/size/frequency profiling
- [ ] Peak RSS, heap usage, retained memory, working set
- [ ] Temporary allocation and object-lifetime analysis
- [ ] Fragmentation indicators
- [ ] Integrations (e.g. heaptrack/dhat/valgrind where available)
- [ ] Optional counting-allocator support
- [ ] Recommendations: reserve capacity, reuse buffers, compact representation
- [ ] `memory` command

### 2.4 Cache analysis (§15)
- [ ] Cache-miss and access-pattern analysis where tooling permits
- [ ] Struct layout analysis (AoS vs SoA candidates)
- [ ] Lower confidence unless runtime evidence supports it

### 2.5 Binary analysis (§19)
- [ ] Binary and section sizes
- [ ] Debug info and symbol counts
- [ ] Duplicated code detection
- [ ] Per-dependency size contribution
- [ ] Recommendations: LTO, strip, feature reduction, consolidation

### 2.6 Compile-time analysis (`tpt-profiler-build`, §8.6, §20)
- [ ] Clean build time
- [ ] Incremental build time
- [ ] Per-crate and dependency build time (`cargo --timings`)
- [ ] Proc-macro and build-script cost
- [ ] Feature-combination cost
- [ ] Build analysis report format (§20 example)
- [ ] `build` command

### 2.7 Richer static detection
- [ ] I/O analysis (§16): repeated reads, small writes, sync I/O, repeated parsing
- [ ] Serialisation analysis (§17): JSON/XML/YAML/binary, repeated ser/de
- [ ] Richer SIMD detection (§12), including image/audio/DSP/crypto kernels
- [ ] Algorithmic complexity heuristics
- [ ] Combine static + runtime evidence to raise confidence

### 2.8 Ecosystem scanning (§33, §34, §60)
- [ ] `ecosystem <path>` command: discover many repos
- [ ] Parallel/incremental multi-repo scan (§72)
- [ ] Shared-opportunity detection across repos (§34)
- [ ] Ecosystem priority weighting with configurable shared-infra value (§60)
- [ ] Ecosystem report (§33 format)
- [ ] Performance tests for scanning 300+ repos, with a memory and time budget
- [ ] Keep aggregate output readable at 300+ repos (top-N, grouping by category, shared-primitive detection)

#### 2.8a GitHub clone → analyse → store → confirm-delete workflow
- [ ] `ecosystem --github <user|org>`: list repos via `gh`/git (explicit opt-in; the network step is never implicit, §70)
- [ ] Repo filters: skip non-Rust, forks, archived; include/exclude globs
- [ ] Shallow-clone into a managed cache dir (configurable location)
- [ ] Mark tool-created clones with a marker file
- [ ] Analyse each clone, static-only by default
- [ ] Runtime/bench on clones is opt-in only, sandboxed or confirmed (executes untrusted build.rs/benches)
- [ ] Store results per repo, keyed by repo + commit SHA, in `.tpt-profiler/`, independent of the clone
- [ ] Regenerate reports from stored results without the source present
- [ ] Cleanup step: list clones with sizes, delete only on confirmation (interactive prompt; `--yes` / `--keep` for non-interactive)
- [ ] Deletion safety: only tool-created clones, never user checkouts, refuse paths outside the managed cache dir
- [ ] Batch processing with a configurable disk budget, so 300+ clones are never on disk at once
- [ ] Resume after interruption: skip repos already analysed at the same SHA
- [ ] Bounded parallelism, per-repo timeouts, partial-failure tolerance (one broken repo never aborts the run)
- [ ] Auth: reuse the user's `gh`/git credentials, never store tokens; handle rate limits
- [ ] Handle repos with no `Cargo.toml`, workspaces inside subdirectories, and name collisions
- [ ] Tests with mocked `gh`/git and a fixture of small local repos

### 2.9 Quality
- [ ] Incremental static analysis cache (§72)
- [ ] Tests for each backend, with mocks where the tool is absent
- [ ] Release v0.2.0

---

## Phase 3 — Workflow, CI & Tracking (§64)

### 3.1 CI mode (§39)
- [ ] `ci` command
- [ ] `[ci]` config: `fail_on_regression`, `runtime_threshold`, `binary_threshold`, etc.
- [ ] Policies: fail on runtime regression, warn on binary size, warn on new HIGH opportunity, fail on critical benchmark regression
- [ ] CI-friendly output (GitHub Actions annotations, summary)
- [ ] Exit codes per policy outcome
- [ ] Example workflow files for GitHub Actions

### 3.2 Regression detection (§37)
- [ ] Per-function / per-benchmark regression detection
- [ ] Severity and confidence for regressions
- [ ] Noise handling and thresholds
- [ ] Regression report format

### 3.3 Output formats
- [ ] JSONL output (§42)
- [ ] HTML report (§42)
- [ ] Stable, versioned schema with compatibility tests (§69)

### 3.4 Persistent opportunity tracking (§38, §52)
- [ ] Persist opportunities with stable IDs across runs
- [ ] Match opportunities between runs (same location/category)
- [ ] Lifecycle transitions: Open → Accepted → In Progress → Optimised → Validated / Regressed / Superseded
- [ ] Validation workflow: detect → recommend → optimise → profile → compare → validate
- [ ] False-positive management commands: dismiss, snooze, ignore, mark-not-applicable, mark-accepted
- [ ] Record reasons for rejection
- [ ] Snooze expiry
- [ ] Store state in `.tpt-profiler/` (git-friendly)

### 3.5 Cross-repository aggregation
- [ ] Aggregated ecosystem opportunity model
- [ ] Opportunity relationships across repos (§35)
- [ ] Shared primitive recommendations (§34)

### 3.6 Integrations (optional, directional, no hard dependency, §43–47)
- [ ] `tpt-weave`: export format consumable for engineering plans
- [ ] `tpt-readme`: performance-status export (§45)
- [ ] `tpt-cargo-vitals`: machine-readable exchange (§46)
- [ ] Integration contract tests with fixtures (no dependency on the other repos)

### 3.7 Release
- [ ] Docs and migration notes
- [ ] Release v0.3.0

---

## Phase 4 — Advanced Analysis (§65)

### 4.1 WASM (§21)
- [ ] WASM binary size analysis
- [ ] Unsupported-dependency detection
- [ ] Allocation behaviour in WASM
- [ ] JS boundary crossing detection
- [ ] WASM SIMD availability
- [ ] Native-only assumption detection
- [ ] Recommendations (§21)

### 4.2 GPU (§22)
- [ ] GPU-suitability indicators and strong-evidence threshold
- [ ] Branch divergence / workload size / utilisation heuristics
- [ ] GPU opportunity report

### 4.3 Energy (§56)
- [ ] CPU/GPU energy counters (RAPL etc. where available)
- [ ] Performance-per-watt and energy-per-operation metrics
- [ ] Energy as an impact dimension

### 4.4 Architecture analysis (§57)
- [ ] Repeated work across modules
- [ ] Unnecessary conversion layers
- [ ] Duplicate implementations
- [ ] Excessive synchronisation
- [ ] Pipeline structure issues
- [ ] Cross-crate inefficiency
- [ ] Mark as lower confidence than measurements

### 4.5 Concurrency analysis
- [ ] Lock contention and excessive-synchronisation detection
- [ ] Thread utilisation / underused cores

### 4.6 Learning & history
- [ ] Learn from dismissals/acceptances to tune heuristics (§52)
- [ ] Track optimisation effectiveness over time (§65)
- [ ] Success metric: how often the top recommendation yields a worthwhile improvement (§80)

### 4.7 Plugin architecture (§67)
- [ ] Stable analyzer plugin interface
- [ ] Loading mechanism (compiled-in features first; external later)
- [ ] Plugin documentation and an example analyzer

### 4.8 Secondary language support (§3.2)
- [ ] Evaluate C/C++, Python analysis feasibility
- [ ] `tpt-simd` awareness: suggest as implementation source when present (§44), still optional
- [ ] `tpt-telos` hook points for equivalence verification (§47)

### 4.9 Release
- [ ] Release v0.4.0

---

## Phase 5 — Experimental (§66, kept outside the core)

- [ ] Decide whether any of this belongs in the core (default: separate system)
- [ ] Automatic experiment generation
- [ ] Candidate optimisation generation
- [ ] Automated benchmark comparison of candidates
- [ ] AI coding-system integration (explicit, opt-in; see privacy §70)
- [ ] Verified optimisation workflow with `tpt-telos`
- [ ] Preserve the safety rule: no automatic source modification in the core profiler (§53)

---

## Cross-cutting (ongoing)

### Release & publishing
- [ ] Release checklist doc
- [ ] Publish crates to crates.io in dependency order (model first)
- [ ] Verify `cargo install tpt-profiler-cli` works on all 3 OSes
- [ ] Prebuilt binaries via CI releases
- [ ] SemVer policy; JSON schema versioning policy

### Quality & performance
- [ ] Keep profiler efficient: few allocations, no network, no heavy DB (§72)
- [ ] Profiler benchmark suite tracked over time
- [ ] Keep static analysis deterministic (§75)
- [ ] Keep the regression corpus growing
- [ ] Periodic dependency and licence audit (cargo-deny)

### Documentation
- [ ] User guide (scan → full → baseline → compare workflow, §77)
- [ ] Scoring model explainer (impact/confidence/effort/risk/generality)
- [ ] How to read evidence and confidence
- [ ] Config reference
- [ ] Contributor guide for adding analyzers

### Principles to check each release
- [ ] Measure first, recommend second, optimise third, measure again (§81)
- [ ] Source is never uploaded and no telemetry is sent (§70)
- [ ] Core never modifies source (§53)
- [ ] Core works without any other TPT repo installed (§3.1.17)
