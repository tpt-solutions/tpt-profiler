# CLI reference

`tpt-profiler` answers *which parts of a project are worth optimising next,
why, by how much, and with what confidence* (spec §1, §41).

```text
tpt-profiler [GLOBAL FLAGS] <COMMAND> [ARGS]
```

## Global flags

| Flag | Default | Meaning |
|------|---------|---------|
| `--path <PATH>` | `.` | Project to analyse. Discovery walks upward from here to the Cargo workspace root. |
| `--format <terminal\|json\|markdown>` | `terminal` | Output format for report-producing commands (spec §42). |
| `--output <FILE>` | stdout | Write the report to a file instead of stdout. |
| `--config <FILE>` | discovered | Use this `tpt-profiler.toml` instead of discovering one (see [config.md](config.md)). |
| `--quiet` | off | Suppress informational output and warnings on stderr. Also disables colour. |
| `--verbose` | off | Print diagnostics (file counts, parse errors, dependency findings) on stderr. |

Colour is disabled when `NO_COLOR` is set (any value) or `--quiet` is given.
Terminal width comes from `COLUMNS` (minimum 40; default 100).

## Commands

### `scan`

Discovers and prints the project structure: repository → project → crates,
with each crate's targets (`lib`, `bin`, `bench`, `test`, …) (spec §7).
Does not analyse code. With `--format json`, prints the repository structure
under the schema envelope.

### `static`

Runs the static analysers (allocation, clone, data-structure, loop, SIMD,
parallelism, unsafe-region tagging, numerical-context recognition) and the
dependency-graph analysis, then ranks the result as opportunities
(spec §8.1, §9–13, §18).

### `full`

Runs every analysis that the MVP supports. In v0.1 this is the same
static-side pipeline as `static`; it also prints the project tree to stderr
unless `--quiet` is given. Runtime and benchmark execution are separate
commands (`runtime`, `bench`) because they run project code.

### `runtime`

Executes the `[[workloads]]` declared in configuration and reports wall-time
measurements with their confidence (spec §8.2, §23, §50). Each workload is
run `repetitions` times after `warmup_runs` discarded runs; a single run is
always reported as low confidence.

If no workloads are configured the command says so and exits `0`.

### `bench`

Runs `cargo bench` for the workspace and ingests Criterion results from
`target/criterion/**/new/estimates.json` (spec §8.3). If no bench targets
exist, or Criterion produced no data, the command reports that as a result
and exits `0`.

### `baseline [NAME]`

Saves the current analysis as a named baseline under
`.tpt-profiler/baselines/<NAME>.json` (default name `default`) (spec §36).

### `compare [NAME]`

Compares the current measurements against a named baseline. Each metric is
reported with its percentage change and one of `improved`, `REGRESSED`,
`unchanged`, or `noisy — not judged`. A pair where either side has low
confidence is never called a regression (spec §36, §50).

Exits `1` with guidance if the baseline does not exist.

### `report [NAME]`

Prints stored results. Only `latest` is available in v0.1; it re-runs the
static analysis in-process. Persisted runs arrive with opportunity tracking
(spec §38, Phase 3).

### `opportunities [--strategy highest-value] [--top N]`

Ranks opportunities and shows the top `N` (default 10) (spec §41, §59).
`highest-value` orders by expected value: priority × confidence × generality
over effort × risk. The priority formula itself is not a public contract
(spec §27).

### `explain <ID>`

Prints the full reasoning for one opportunity: scores, interpretation,
recommendation, evidence, and the scoring trace (spec §41). The ID may be a
unique prefix, e.g. `ALLOCATION-0514`.

### `completions <SHELL>`

Prints a shell completion script for `bash`, `zsh`, `fish`, `powershell`, or
`elvish`.

```sh
tpt-profiler completions bash > ~/.local/share/bash-completion/completions/tpt-profiler
```

### `man`

Prints the man page in roff format.

```sh
tpt-profiler man > tpt-profiler.1
```

## Output formats

* **`terminal`** — ranked list with colour (unless disabled), each entry
  labelled `Basis: deterministic (static analysis)` or
  `Basis: statistical (measured)` (spec §29).
* **`markdown`** — a ranked table followed by one section per opportunity.
* **`json`** — the schema envelope; see [json-schema.md](json-schema.md).

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | The command ran. This includes empty results such as *no opportunities detected* or *no benchmarks found* (spec §74). |
| `1` | The command failed: invalid configuration, missing tool (e.g. `cargo`), corrupt baseline, unknown ID, or a benchmark that did not run successfully. |

Warnings (for example, parse errors in individual source files) go to stderr
and do not change the exit code.

## Examples

```sh
tpt-profiler scan
tpt-profiler static --format markdown --output report.md
tpt-profiler opportunities --top 5 --format json
tpt-profiler explain SIMD-0421
tpt-profiler runtime
tpt-profiler baseline before-refactor
# ... make changes ...
tpt-profiler runtime
tpt-profiler compare before-refactor
```
