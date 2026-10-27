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
| `--format <FORMAT>` | `terminal` | Output format for report-producing commands: `terminal`, `json`, `jsonl`, `markdown`, `html`, `github-annotations` (spec §42). |
| `--output <FILE>` | stdout | Write the report to a file instead of stdout. |
| `--summary <FILE>` | none | Append a Markdown summary of the results to a file, separate from `--format`. In GitHub Actions, point it at `$GITHUB_STEP_SUMMARY` (spec §39). |
| `--config <FILE>` | discovered | Use this `tpt-profiler.toml` instead of discovering one (see [config.md](config.md)). |
| `--quiet` | off | Suppress informational output and warnings on stderr. Also disables colour. |
| `--verbose` | off | Print diagnostics (file counts, parse errors, dependency findings) on stderr. |
| `--disable <MODE>[,<MODE>…]` | none | Turn off modes for this run, overriding the config file: `runtime`, `memory`, `cpu`, `simd`, `parallelism`, `benchmarks`, `ci`. Only disables. |

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
parallelism, algorithmic complexity, I/O, serialisation, unsafe-region tagging,
numerical-context recognition) and the dependency-graph analysis, then ranks
the result as opportunities (spec §2, §8.1, §9–18).

Unchanged files are not analysed again. Findings are cached per file, keyed
by a hash of the file's contents, in `.tpt-profiler/cache/static-cache.json`
(spec §72). The cache is only a speed-up: deleting it changes timing, never
results. `--verbose` reports how many files came from the cache. Ecosystem
scans never write a cache into the repositories they read.

### `full`

Runs every analysis that the MVP supports. In v0.1 this is the same
static-side pipeline as `static`; it also prints the project tree to stderr
unless `--quiet` is given. Runtime and benchmark execution are separate
commands (`runtime`, `bench`) because they run project code.

`full --save NAME` also stores the run under `.tpt-profiler/runs/NAME.json`,
so `report NAME` can print it later without re-analysing the source. Names may
use letters, digits, `-` and `_` only.

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

Each benchmark's mean is a measurement named `<benchmark>_bench_mean`. Its
confidence comes from the relative half-width of Criterion's 95% interval on
the mean: `High` up to 2%, `Medium` up to 5%, `Low` beyond that (spec §50).
`baseline`, `compare`, and `ci` include these measurements as well.

### `baseline [NAME]`

Saves the current analysis as a named baseline under
`.tpt-profiler/baselines/<NAME>.json` (default name `default`) (spec §36).

### `compare [NAME]`

Compares the current measurements against a named baseline. Each metric is
reported with its percentage change and one of `improved`, `REGRESSED`,
`unchanged`, or `noisy — not judged`. A pair where either side has low
confidence is never called a regression (spec §36, §50).

A regression carries a severity and a confidence. Severity is how far the
change overshoots the threshold: `minor` below 2× the threshold, `major` from
2×, and `critical` from 5× (for example, a `runtime_threshold` of `0.10` makes
a 25% slowdown major). Confidence is the weaker of the baseline and current
measurements (spec §37).

Exits `1` with guidance if the baseline does not exist.

### `ci [NAME]`

Runs the configured `[[workloads]]` (spec §23) and benchmarks (when
`[benchmarks] enabled`), then applies the `[ci]` policy to the comparison
against a named baseline (default `default`) (spec §39):

* a regressed metric **fails** the run when `fail_on_regression = true`, and
  warns otherwise;
* a metric whose measurements are low confidence (for example, few samples or
  high spread) is noted as noisy and never judged, so a noisy machine cannot
  fail a build;
* a workload that fails or times out is a warning, since the comparison cannot
  cover it;
* when `[profiling] binary = true`, binary growth above `binary_threshold` is
  a warning and never fails the run (spec §39);
* an opportunity that is new since the `baseline` record and has priority at
  or above `[ci] high_priority` (default 70) is a warning. Dismissed and
  snoozed opportunities stay quiet, and so do opportunities already in the
  record. If no record exists, `ci` notes that instead (spec §39);
* an environment mismatch between baseline and current is a warning.

`baseline` and `compare` run the same workloads, so the baseline and the
comparison always use the same metric names.

**Note:** workloads execute project code. Run `ci` only on trusted branches.

Exits `1` when any outcome fails the run. A missing baseline is an error, not
a pass, so a misconfigured pipeline cannot go green silently. With
`enabled = false` in `[ci]` the command reports that and exits `0`.

With `--format github-annotations`, each outcome is printed as a `::error::`,
`::warning::`, or `::notice::` workflow command.

Not yet implemented: the policy's "warn on a new HIGH-priority opportunity"
rule, which needs persisted opportunity tracking (Phase 3).

### `report [NAME]`

Prints the full report. `latest` re-runs the static analysis in-process and
applies the recorded decisions from `.tpt-profiler/opportunities.json`, so
opportunities that were dismissed or are still snoozed are hidden and each one
shows its lifecycle state (spec §38). Any other name prints a run saved by
`full --save NAME`, with the same decisions applied, and does not re-analyse
the source.

### `opportunities [--strategy highest-value] [--top N]`

Ranks opportunities and shows the top `N` (default 10) (spec §41, §59).
`highest-value` orders by priority, highest first, with ties broken by ID. The
priority is computed from impact, confidence, generality, effort and risk; see
[the scoring model](scoring.md). The formula itself is not a public contract
(spec §27).

### `dismiss <ID> [--reason TEXT]`

Records that a finding is a false positive or not worth doing. The reason is
kept in `.tpt-profiler/opportunities.json`. A dismissed opportunity is hidden
from `opportunities` and never reported as new by `ci` (spec §38).

### `mark-not-applicable <ID> --reason TEXT`

Records that the finding's category does not apply to this code, for example
SIMD advice in code with no numeric kernels. It is recorded as a rejection
whose reason starts with `not applicable:`, and it is hidden like a dismissal
(spec §52).

### `ignore <ID>`

Hides a finding without judging whether it is real. Unlike `dismiss`, it
records no rejection reason and leaves the lifecycle state as it was. Use
`reopen` to show it again (spec §52).

### `accept <ID>`

Records that a finding is real and will be worked on (spec §38).

### `snooze <ID> [--days N]`

Hides a finding from `opportunities` for N days (default 30). It reappears
automatically when the snooze expires (spec §38).

### Progress: `start`, `optimise`, `validate`, `regress`, `supersede`

These record the work that follows a finding (spec §38). Each one checks the
current state and is refused, with nothing recorded, when the order is wrong:

| Command | Moves to | Allowed from |
|---|---|---|
| `start <ID>` | in progress | open, accepted |
| `optimise <ID>` | optimised | in progress |
| `validate <ID>` | validated | optimised |
| `regress <ID>` | regressed (spec §37) | optimised |
| `supersede <ID> --by <OTHER>` | superseded by `OTHER` | any state, except already superseded |

`validate` and `regress` also happen on their own: `compare <NAME>` moves an
optimised opportunity to **validated** when one of its measured metrics
improved, and to **regressed** when one of them got worse. Metrics that are
unchanged or noisy leave it optimised. `ci` only reads the record, so a pull
request never changes it.

`supersede` requires `OTHER` to be a recorded opportunity different from `ID`.
`accept` is refused while work is in progress, optimised, validated,
regressed or superseded, so triage cannot wipe progress; `reopen` returns the
opportunity to open first.

### `cpu`

Runs each `[[workloads]]` entry under Linux `perf` (spec §8.5). It reports the
mean of the hardware counters (instructions, cycles, branches, branch misses,
cache references and misses) over three runs, then the top leaf frames from a
sampling recording at 499 Hz.

Each hotspot is followed by its source location when its symbol names exactly
one function in the project, for example `(src/lib.rs:12)`. Ambiguous or
unnamed frames such as closures are left without a location.

Counters are only available on Linux with `perf` on `PATH`. Anywhere else the
command prints `CPU counters unavailable` and exits `0`, so the rest of the
pipeline is unaffected. Windows (ETW/WPR) and macOS (`xctrace`) backends are
not implemented yet.

### `build`

Times two `cargo build --workspace --timings` runs (spec §8.6, §20). The first
builds from an empty directory, giving the clean time. The second follows a
touch of one workspace crate's root source file, giving the incremental time.
The touch changes only the file's modification time, which is restored
afterwards; the file's contents are never written.

Both builds use `.tpt-profiler/build-target`, which the command clears first.
Your own `target/` directory is never touched. Ignore that directory in version
control; this repository's `.gitignore` already does.

The report lists the time split between library, build-script and proc-macro
units, the ten most expensive crates, and findings where cost concentrates:
build scripts or proc-macros above 10% and 15% of unit time, and any single
crate above 25%. Unit times are summed over parallel jobs, so shares describe
where compile work goes, not how long the build took.

Every finding is `Low` confidence because each build is a single timed run.
`--format json` prints the same data under the schema envelope: clean and
incremental seconds, the per-kind split, every crate, the findings and the
measurements that a baseline would store.

Feature-combination cost (spec §20) is not measured yet.

### `memory`

Runs each `[[workloads]]` entry once and reports its peak resident memory
(spec §8.4, §14). The value is sampled from `/proc/<pid>/status` (`VmHWM`) about
every 10 ms while the workload runs, and the largest reading is kept.

The result is a lower bound. A workload that allocates heavily in its last few
milliseconds and exits before the next poll can read less than its true peak.
Allocation counts and sizes need a counting allocator, which the workspace
cannot ship because it forbids `unsafe`; they are not reported.

Linux only. Elsewhere the command prints `peak memory unavailable` and exits
`0`. The measurement is `memory:<workload>_peak_rss_bytes`, at Low confidence
because it is one run. `--format json` prints the same data under the schema
envelope.

### `energy`

Runs each `[[workloads]]` entry once between two readings of the Linux RAPL
package counters under `/sys/class/powercap` (spec §56). Each top-level
`intel-rapl:<n>` zone is summed, and a counter wrap is handled through its
`max_energy_range_uj`.

The counter is system-wide, so any other load on the machine during the run is
included. Treat the value as an upper bound with Low confidence. Recent kernels
make the counter readable only by root. When it cannot be read, the command
prints `package energy unavailable` and exits `0`. GPU energy and
performance-per-watt are not reported.

The measurement is `energy:<workload>_joules`.

### `effectiveness`

Summarises the recorded decisions and outcomes by category (spec §52, §65,
§80): how many opportunities were seen, dismissed, accepted, optimised,
validated and regressed, with a dismissal rate and a success rate. The success
rate is validated work as a share of all judged optimised work.

This is a report over `.tpt-profiler/opportunities.json`. It does not change
any heuristic or score. Tuning stays a manual decision, based on these rates.

### `ecosystem [ROOT] [--max-depth N] [--shared-weight F] [--top N] [--workers N]`

Scans every Rust repository under `ROOT` (default: `--path`) and reports what
they share (spec §33, §34, §60). A repository is any directory with a
`Cargo.toml`; the search stops there, so workspace members belong to their
repository. `target`, `node_modules` and hidden directories are skipped.

Repositories are analysed on `--workers` threads (default: the machine's
parallelism, at most 8). The report is the same whatever the worker count.

The scan is static and local only: no `cargo metadata`, no registry access and
no workloads. A repository whose `Cargo.toml` is not valid TOML, or that cannot
be read, is listed with its error and the others still run.

Shared opportunities are findings with the same category and text in two or
more repositories. Their score is the best priority, raised by
`--shared-weight` (default `0.5`) for each extra repository. Three or more
repositories get a suggestion to share one helper or primitive (spec §34).
`--format json` prints the whole report under the schema envelope.

### `ecosystem --github OWNER [--include GLOB]… [--exclude GLOB]… [--max-repos N] [--disk-budget-mb MB] [--cache-dir DIR] [--yes | --keep]`

Lists `OWNER`'s repositories on GitHub, analyses the Rust ones and stores the
results (spec §70). This is the only command that touches the network, and it
runs only when you name an owner. It uses `gh` and `git` as already logged in;
no token is read or stored by the tool.

Steps, in order:

1. `gh repo list` lists the owner's repositories. Forks, archived repositories,
   repositories not detected as Rust, and repositories matching `--exclude`
   are skipped. With `--include`, only matching repositories are kept. A
   pattern with a slash matches `owner/name`; one without matches the name.
2. `git ls-remote` reads each head commit without cloning. A result already
   stored for that commit is reused, so an interrupted or repeated run does not
   clone again.
3. The rest are shallow-cloned (`--depth 1`) into the clone directory, at most
   `--max-repos` in all. Clones are made in batches whose estimated size fits
   `--disk-budget-mb` (default 2048). GitHub's reported size is the estimate.
4. Each result is stored in `.tpt-profiler/github/<owner>__<name>@<commit>.json`,
   independent of the clone. A repository whose analysis failed is not stored,
   so the next run retries it. A repository with no `Cargo.toml` is stored as
   `no_manifest`.
5. Clones are deleted only when they carry the marker file this tool writes
   and sit directly under the clone directory. Anything else there is refused.

Cleanup is decided by the flags. With `--yes`, clones are deleted when the
budget is reached and at the end of the run. With `--keep`, clones stay, and a
run that reaches the budget stops and says so. With neither, a terminal run asks
before each deletion, and a non-terminal run keeps the clones.

`--from-store` reports the stored results for `OWNER` with no network and no
clones. It keeps the newest commit per repository. That is how a report is
regenerated after the source has gone.

Repository failures are listed and do not stop the run. The report shows
opportunities shared across repositories, as in the local scan.

### `reopen <ID>`

Forgets a dismissal, acceptance, snooze, or progress state (spec §38).

`dismiss`, `ignore`, `mark-not-applicable`, `accept`, `snooze`, `start`,
`optimise`, `validate`, `regress`, `supersede` and `reopen` accept a unique ID
prefix. An ambiguous or unknown prefix is an error, and nothing is recorded.

### Opportunity record

`baseline` observes the current opportunities into `.tpt-profiler/opportunities.json`,
so later `ci` runs can tell what is *new* since the reference branch. `ci`
only reads this file; a pull request does not change the record it is
compared against. Commit the file to share decisions with the team.

The record is consulted by `opportunities`, which applies stored decisions and
hides dismissed and snoozed items. `static` and `full` list every finding.

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
* **`jsonl`** — one self-describing JSON object per opportunity per line,
  for streaming and `jq` pipelines.
* **`html`** — one self-contained page with no scripts or external resources.
* **`github-annotations`** — one `::notice` workflow command per opportunity,
  attached to its source file and line where it has one.

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
