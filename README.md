# tpt-profiler

> **Which parts of this project are worth optimising next, why, by how much, and with what level of confidence?**

`tpt-profiler` is a repository and workload optimisation analysis system for
identifying, measuring, ranking, and explaining opportunities to improve
software performance, efficiency, and architecture.

It is not just another profiler. Traditional profilers answer *"what is
consuming resources?"* — `tpt-profiler` answers *"what should I change?"*
by combining static analysis, runtime profiling, benchmark analysis, and
dependency analysis into ranked, evidence-backed **opportunities**.

**Status:** design implementation in progress (`spec.txt` is the source of
truth). v0.1.0 targets the MVP scope of spec §62.

## Planned features

* Cargo workspace / crate discovery (`tpt-profiler scan`)
* Static analysis: allocation, clone, data-structure, SIMD, and
  parallelism candidates — each with evidence and confidence
* Runtime and benchmark measurement (Criterion integration where available)
* Opportunity engine: impact, confidence, effort, risk, generality, priority
* Baselines and regression comparison (`baseline`, `compare`)
* Terminal, JSON, and Markdown reports
* Fully local operation: no telemetry, no source upload, ever

## Quick start

From a checkout (crates.io release pending):

```bash
cargo install --path crates/tpt-profiler-cli

tpt-profiler scan                        # discover the project structure
tpt-profiler static                      # rank static-analysis opportunities
tpt-profiler opportunities --top 5       # highest-value first
tpt-profiler explain ALLOCATION-0514     # full reasoning; prefixes work

tpt-profiler --format json opportunities # machine-readable, schema-versioned
```

Runtime and benchmark measurements are opt-in because they run project code:

```bash
tpt-profiler runtime                     # needs [[workloads]] in tpt-profiler.toml
tpt-profiler bench                       # runs `cargo bench`, reads Criterion output
tpt-profiler baseline before-change      # save measurements
tpt-profiler compare before-change       # % change, noise-aware
```

Documentation: [CLI reference](docs/cli.md) · [configuration](docs/config.md) ·
[JSON schema](docs/json-schema.md). Shell completions and a man page come
from `tpt-profiler completions <shell>` and `tpt-profiler man`.

## Repository layout

```text
crates/
  tpt-profiler/           core library (orchestration pipeline)
  tpt-profiler-cli/       command-line interface
  tpt-profiler-model/     shared data model (no_std-capable)
  tpt-profiler-static/    static analysis engine
  tpt-profiler-runtime/   runtime profiling
  tpt-profiler-bench/     benchmark analysis
  tpt-profiler-memory/    memory analysis
  tpt-profiler-build/     build/compile-time analysis
  tpt-profiler-opportunity/ opportunity engine & scoring
  tpt-profiler-report/    output/report generation
tests/                    cross-crate integration tests and fixtures
```

See [DESIGN.md](DESIGN.md) for architecture and crate dependency rules,
[CHANGELOG.md](CHANGELOG.md) for release history, and
[spec.txt](spec.txt) for the full design specification.

## MSRV

The minimum supported Rust version is **1.85** (the first release with
edition 2024 support). Both a current stable toolchain and the MSRV are
tested in CI.

## Licence

`tpt-profiler` is dual-licensed under the [MIT](LICENSE-MIT) and
[Apache 2.0](LICENSE-APACHE) licences; you may choose either.
See [LICENSE](LICENSE) for details. Contributions are licensed under both.
