# Configuration reference

Configuration lives in `tpt-profiler.toml` (spec §40). It is optional: with no
file, every default below applies.

## Discovery and precedence

* The file is found by searching upward from `--path` (default `.`), stopping
  at the first `tpt-profiler.toml`. Use `--config <FILE>` to name one
  explicitly; discovery is then skipped.
* Precedence is **flags > config file > built-in defaults** (spec §40).
* Unknown keys and sections are errors, so typos fail loudly with the file
  and line rather than being silently ignored.

## Example

```toml
[project]
name = "my-service"           # default: directory name

[profiling]
runtime = true
memory = true
cpu = true
simd = true
parallelism = true

[benchmarks]
enabled = true

[ci]
enabled = true
fail_on_regression = true
runtime_threshold = 0.10      # fail above a 10% slowdown
binary_threshold = 0.05       # warn above 5% growth

[scoring]
impact_weight = 1.0
confidence_weight = 1.0
generality_weight = 1.0
effort_weight = 1.0
risk_weight = 1.0

[[workloads]]
name = "decode-1080p"
command = "target/release/decoder"
args = ["fixtures/1080p.bin"]
repetitions = 5               # default 5
warmup_runs = 2               # default 2
timeout_ms = 60000            # default 60000
```

## `[project]`

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `name` | string | directory name | Repository name shown in reports and used in opportunity IDs. |

## `[profiling]`

| Key | Type | Default | Status in v0.1 |
|-----|------|---------|----------------|
| `runtime` | bool | `true` | When `false`, the `runtime` command reports that it is disabled and runs no workloads. |
| `memory` | bool | `true` | Parsed. Memory profiling is Phase 2. |
| `cpu` | bool | `true` | Parsed. CPU profiling is Phase 2. |
| `simd` | bool | `true` | When `false`, SIMD findings are dropped from `static`, `full`, and `opportunities`. |
| `parallelism` | bool | `true` | When `false`, parallelism findings are dropped from the analysis. |

`memory` and `cpu` are reserved until their analyses exist (Phase 2). Every toggle can be
turned off for one run with `--disable <mode>`.

## `[benchmarks]`

| Key | Type | Default | Status in v0.1 |
|-----|------|---------|----------------|
| `enabled` | bool | `true` | When `false`, the `bench` command reports that benchmarks are disabled and runs nothing. |

## `[ci]`

| Key | Type | Default | Status in v0.1 |
|-----|------|---------|----------------|
| `enabled` | bool | `true` | Reserved: there is no `ci` command until Phase 3. |
| `fail_on_regression` | bool | `true` | Parsed. Applies to CI mode (Phase 3). |
| `runtime_threshold` | float | `0.10` | Used by `compare` to decide what counts as a regression (fraction: `0.10` = 10%). |
| `binary_threshold` | float | `0.05` | Reserved for binary-size analysis (Phase 2). |

## `[scoring]`

Multipliers for the priority score (spec §27). They tune how strongly each
factor counts; the formula's shape is deliberately not part of the public
contract (`DESIGN.md` §7). Each weight must be finite and greater than zero.

| Key | Default | Factor |
|-----|---------|--------|
| `impact_weight` | `1.0` | Impact (spec §28) |
| `confidence_weight` | `1.0` | Confidence (spec §29) |
| `generality_weight` | `1.0` | Generality (spec §32) |
| `effort_weight` | `1.0` | Effort divisor (spec §30) |
| `risk_weight` | `1.0` | Risk divisor (spec §31) |

## `[[workloads]]`

Each entry describes one measured run (spec §23). Used by `tpt-profiler runtime`.
Several entries may be given.

| Key | Type | Required | Default | Meaning |
|-----|------|----------|---------|---------|
| `name` | string | yes | — | Stable name used in measurements and baselines. |
| `command` | string | yes | — | Executable. Relative paths resolve against `--path`. |
| `args` | array of strings | no | `[]` | Arguments, typically including the input. |
| `repetitions` | integer | no | `5` | Measured runs. |
| `warmup_runs` | integer | no | `2` | Runs discarded before measuring. When at least one warm-up run is set, the first one is also recorded separately as the cold-start observation. |
| `timeout_ms` | integer | no | `60000` | Per-run timeout. A run that exceeds it is killed and counted as failed. |

Workloads execute project code. Only declare commands you trust.
