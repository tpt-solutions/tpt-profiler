# Scoring model

Audience: people reading an opportunity's priority, or tuning `[scoring]`
(spec §27–32).

Each opportunity has five judgements. They combine into one priority from 0
to 100. The priority orders the list; it is not a measurement and not a
promise of speed-up.

## The five factors

| Factor | Scale | Meaning (spec) |
|---|---|---|
| Impact | 1–10 | How much the change could matter if it works (§28) |
| Confidence | Very low 2, Low 4, Medium 6, High 8, Very high 10 | How well the evidence supports the finding (§29) |
| Generality | 1–10 | How many places or projects the change would apply to (§32) |
| Effort | Trivial 1, Low 3, Medium 5, High 7, Very high 9 | How much work the change is (§30) |
| Risk | 1–10 | Overall risk, including correctness, numerics, concurrency and unsafe code (§31) |

The confidence and effort scales are not 1–10. Confidence is always even, and
effort is always odd. The numbers are the factor values the formula uses.

## The formula

```text
raw      = (impact × wᵢ) × (confidence × w𝒸) × (generality × w𝓰)
           ─────────────────────────────────────────────────────
           (effort × wₑ) × (risk × wᵣ)

priority = round( 100 × √(raw / 1000) ),  clamped to 0–100
```

The square root softens the divide by effort and risk. A linear map would
push most realistic candidates close to zero.

Bounds at default weights (all `1.0`):

* Best case: impact 10, confidence 10, generality 10, effort 1, risk 1.
  `raw` = 1000, so priority = 100.
* Worst case: impact 1, confidence 2, generality 1, effort 9, risk 10.
  `raw` ≈ 0.022, so priority ≈ 15.

So priorities at default weights fall between about 15 and 100.

## Weights

`[scoring]` in `tpt-profiler.toml` sets `impact_weight`, `confidence_weight`,
`generality_weight`, `effort_weight` and `risk_weight`. Each must be finite and
greater than zero; anything else fails validation with a message naming the
key (see [config.md](config.md)).

Weights tune how strongly each factor counts. A larger `risk_weight` pushes
risky findings down; a larger `impact_weight` pushes impactful findings up. The
weights do not change the shape of the formula.

## What the number is for

* **Ordering.** `opportunities` sorts by priority, highest first, and breaks
  ties by ID so the order is stable. `--top N` shows the first N.
* **Explanation.** `explain <ID>` prints each factor and its value, so a
  reader can see why a finding ranks where it does (spec §59).

## What it is not

* It is not a speed-up estimate. Impact is a judgement, not a measurement.
* It is not a stable API. The formula can change in any release, which is why
  it is not documented as a contract (spec §27). Reports carry the factors so
  a reader can recompute a score if they need to.
* Confidence comes from the strongest evidence item, capped at that tier's
  ceiling (spec §29): static pattern at `Medium`, heuristic at `Low`, runtime
  profile or counters at `High`, benchmark regression at `Very high`. A static
  pattern cannot reach `High` however many matches it has.
