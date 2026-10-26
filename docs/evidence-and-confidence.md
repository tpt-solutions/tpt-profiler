# Reading evidence and confidence

Audience: anyone deciding whether a profiler finding is worth acting on
(spec §29, §50, §75).

Every opportunity says what it is based on (evidence) and how sure the tool is
(confidence). Read both before deciding. A high priority with weak evidence is
still a guess.

## Two kinds of finding

Each finding is labelled with its basis:

* **static (deterministic)**: the same source always gives the same finding.
  It says where a pattern occurs. It does not show a cost.
* **measured (statistical)**: a timing or size taken from a run. It varies
  between runs, so it comes with a confidence.

Reports use these labels in every output format. Terminal, Markdown, HTML and
GitHub annotations all carry them.

## The evidence hierarchy

The strongest evidence an opportunity has sets how far its confidence can go
(spec §29):

| Evidence | Strength | Confidence ceiling | Where it comes from |
|---|---|---|---|
| Benchmark regression | 5 | Very high | A benchmark got slower against a baseline |
| Runtime profile | 4 | High | Profiler output attributing cost to code |
| Hardware counter | 3 | High | Cycles, cache misses, branches (spec §8.5) |
| Static pattern | 2 | Medium | A source pattern matched, such as allocation in a loop |
| Heuristic | 1 | Low | A guess with no direct observation |

**What the tool produces today.** Only static patterns and heuristics become
opportunities. Runtime profiles, hardware counters and benchmark regressions
are part of the hierarchy, but no profiler backend feeds them into
opportunities yet (Phase 2). Measurements from `runtime` and `bench` are
compared against a baseline by `compare` and `ci`; they do not change an
opportunity's evidence or priority.

Two rules follow from the table:

* **More of the same evidence does not raise confidence.** Ten static matches
  are still `Medium`. Confidence is the ceiling of the strongest item, not a
  count.
* **A static pattern cannot reach `High`,** however clean the code looks. Only
  a measurement can justify `High`.

A static finding at `Medium` detector confidence or above counts as a static
pattern. Weaker ones count as heuristics, with a `Low` ceiling.

## Confidence in measurements

Measured values get their confidence from how much they vary.

**Workloads.** Confidence comes from the coefficient of variation of the timed
runs: the standard deviation divided by the mean.

| Coefficient of variation | Confidence |
|---|---|
| 2% or less | High |
| more than 2%, up to 5% | Medium |
| more than 5% | Low |

Two more rules apply:

* A **single run is always `Low`**. One sample says nothing about spread.
* The **cold-start** time (the first run) is always `Low`, because it is a
  different quantity from steady-state time.

**Benchmarks.** Criterion reports a 95% confidence interval for each mean. The
tool uses the interval's half-width divided by the mean, and applies the same
thresholds: 2% and 5%. It is not a coefficient of variation, so the two numbers
differ a little for the same run.

**Binary size.** Size is deterministic for a given toolchain and source, so it
is `High`. It still changes when the toolchain or dependencies change, which is
why `compare` warns about a different environment.

## What confidence does in comparisons

* A pair where **either side is `Low`** is `noisy — not judged`. It is never a
  regression, and never an improvement.
* A **regression's confidence is the weaker of the two sides.** A `High`
  baseline compared with a `Medium` current run gives a `Medium` regression.
* Severity (`minor`, `major`, `critical`) says how far past the threshold the
  change went. It does not say how sure the tool is. Check both.

## What to do with the numbers

| Situation | Reasonable next step |
|---|---|
| Static only, `Medium` | Look at the code. Measure before changing it. |
| Static only, `Low` | Probably skip unless the code is obviously hot. |
| Measured, `High`, regression `major` | Investigate now. |
| Measured, `Medium` | Repeat the run to tighten the spread before deciding. |
| `noisy — not judged` | Raise `repetitions`, close other work, measure again. |

## Limits

* Static analysis sees syntax, not types or what the optimiser does. A pattern
  may already be optimised away, or may never run.
* Measurements cover only the workloads and benchmarks you provide. A code
  path that no workload exercises is not measured at all, and a measured
  regression does not yet point at the code that caused it.
* Runtime timings depend on the machine. Compare on the same kind of machine,
  and treat a change of operating system as a warning, not a verdict.
* Priority is a judgement built on these numbers, not a measured speed-up. See
  [scoring.md](scoring.md).
