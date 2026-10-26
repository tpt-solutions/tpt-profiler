# User guide: scan, analyse, baseline, compare

Audience: Rust developers who want to find where a project can be faster,
measure a change, and keep a performance check in CI (spec §77).

This guide follows one workflow. The full flag reference is in
[cli.md](cli.md) and every setting is in [config.md](config.md).

## 0. Build the tool

The CLI crate is `tpt-profiler-cli`; its binary is `tpt-profiler`. From a
checkout of this repository:

```sh
cargo build --release -p tpt-profiler-cli
./target/release/tpt-profiler --help
```

Crates.io publication is not done yet, so there is no `cargo install` step.

## 1. Look at the project

```sh
tpt-profiler scan
```

`scan` prints the repository, its projects, and each crate's targets. It
analyses nothing, so it is a quick check that the right crates were found.

Run it from the project root, or pass `--path <DIR>`. Discovery uses the
nearest `tpt-profiler.toml` upward from that path.

## 2. Find opportunities

```sh
tpt-profiler static
tpt-profiler opportunities --top 10
```

`static` analyses source and the dependency graph, then ranks what it found.
`opportunities` shows the top of that ranking and hides anything you have
dismissed, ignored, or snoozed (see step 5).

Each opportunity has an ID such as `SIMD-0421`. Use it with `explain`:

```sh
tpt-profiler explain SIMD-0421
```

`explain` shows each factor behind the priority, the evidence, and the
caveats. Read [scoring.md](scoring.md) for what the numbers mean and
[evidence-and-confidence.md](evidence-and-confidence.md) for how far to trust
them.

Static findings are *patterns*. They say where to look, not that the change
will help. Confirm with measurement before acting on one.

## 3. Measure the code you care about

Static analysis cannot show a speed-up. Measurement can. Two sources:

**Workloads** run a command you choose. Declare one in `tpt-profiler.toml`:

```toml
[[workloads]]
name = "decode_1080p"
command = "target/release/examples/decode"
args = ["fixtures/1080p.bin"]
repetitions = 10
warmup_runs = 2
```

Build the command first, then measure:

```sh
tpt-profiler runtime
```

Each workload runs `warmup_runs` times and is then timed `repetitions` times.
With no workloads configured, `runtime` says so and exits `0`.

**Benchmarks** use Criterion or `cargo bench`:

```sh
tpt-profiler bench
```

If the workspace has no bench targets, this reports that and exits `0`.

Measurements carry a confidence. A single run is always `Low` confidence. See
[evidence-and-confidence.md](evidence-and-confidence.md).

## 4. Save a baseline, then compare

Before a change, save what the current code does:

```sh
tpt-profiler baseline before-refactor
```

This writes `.tpt-profiler/baselines/before-refactor.json`. It stores the
measurements (workloads, benchmarks, and binary size when `[profiling] binary =
true`) and records the current opportunities so later runs can tell what is new.

Make the change, measure again, and compare:

```sh
tpt-profiler runtime
tpt-profiler compare before-refactor
```

`compare` prints each metric's percentage change with one of:

* `improved`: faster or smaller beyond the threshold;
* `REGRESSED`: slower or bigger beyond the threshold, with a severity
  (`minor`, `major`, `critical`) and a confidence;
* `unchanged`: inside the noise margin;
* `noisy — not judged`: one side has `Low` confidence, so no verdict is given.

The default threshold is 10% (`[ci] runtime_threshold = 0.10`). A noisy
result is never a regression. If you see one, run the workload with more
repetitions, on a quieter machine, before deciding.

`compare` warns when the baseline and the current run were taken on different
operating systems. Compare like with like where you can: the same machine and
toolchain.

## 5. Record decisions

Not every opportunity deserves work. Record the decision so it stops
appearing:

```sh
tpt-profiler dismiss <ID> --reason "hot path is not in the release build"
tpt-profiler ignore <ID>
tpt-profiler mark-not-applicable <ID> --reason "no numeric kernels here"
tpt-profiler accept <ID>
```

`<ID>` is an opportunity ID or a unique prefix of one, as shown by
`opportunities`.

Work through the lifecycle as you go:

```sh
tpt-profiler start <ID>
tpt-profiler optimise <ID>   # after the change lands
tpt-profiler validate <ID>   # after compare shows the gain
```

Each step is checked; `start` must come before `optimise`, and so on. See
[cli.md](cli.md) for the full table.

Decisions live in `.tpt-profiler/opportunities.json`. Commit that file to
share them with the team.

## 6. Check in CI

Add a baseline on the main branch, commit `.tpt-profiler/baselines/`, and run
`ci` on each pull request:

```sh
tpt-profiler ci main
```

`ci` runs the workloads and benchmarks, compares against the named baseline,
and applies the `[ci]` policy:

* a regressed metric fails the run when `fail_on_regression = true` (exit
  `1`), and only warns when it is `false`;
* a noisy metric is noted and never fails the run;
* a binary size growth beyond `binary_threshold` warns and never fails;
* a new opportunity with priority at or above `high_priority` (default `70`)
  warns.

Write the output for GitHub Actions with `--format github-annotations`, and
append a Markdown summary with `--summary <FILE>`. A ready workflow is in
[examples/github-actions.yml](examples/github-actions.yml).

Exit codes are listed in [cli.md](cli.md#exit-codes). `0` means the command
ran, including empty results; `1` means it failed or a policy failed.

## Common problems

* **"no comparable measurements".** `compare` and `ci` need workloads or
  benchmarks. Run `runtime` or `bench` first, or add `[[workloads]]`.
* **"baseline ... not found".** Create it with `baseline <name>` on the
  reference revision.
* **"no opportunities detected".** That is a result, not an error. Your code
  matched none of the patterns, or your decisions hid them all.
* **Every measurement is noisy.** Repetitions are too few, or the machine is
  busy. Raise `repetitions` and close other work.
* **Parse errors in some files.** They are reported on stderr and analysis
  continues with the rest.
