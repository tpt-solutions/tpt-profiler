# JSON output schema

Every JSON artefact `tpt-profiler` writes is wrapped in a versioned envelope
(spec §69), so downstream tools can check what they are reading before they
parse it.

## Envelope

```json
{
  "schema_version": 1,
  "generated_by": "tpt-profiler 0.1.0",
  "data": { }
}
```

| Field | Type | Meaning |
|-------|------|---------|
| `schema_version` | integer | Version of the envelope and data shapes. Currently `1`. |
| `generated_by` | string | Tool name and version that wrote the file. |
| `data` | object | The artefact (see below). |

Readers must reject a `schema_version` they do not understand. The profiler
itself refuses to read baselines written by a newer schema (`SchemaTooNew`),
and never reinterprets them.

## Opportunity summary (`static`, `full`, `opportunities`)

`--format json` on the analysis commands produces:

```json
{
  "schema_version": 1,
  "generated_by": "tpt-profiler 0.1.0",
  "data": {
    "repository": "my-service",
    "opportunities": [
      {
        "id": "ALLOCATION-05294",
        "category": "ALLOCATION",
        "component": "src/lib.rs",
        "impact": 5,
        "confidence": 6,
        "effort": 5,
        "risk": 5,
        "generality": 6,
        "priority": 8
      }
    ]
  }
}
```

`opportunities` is in rank order. With no findings it is an empty array, not
a prose message, so the output is always parseable.

| Field | Type | Scale and meaning |
|-------|------|-------------------|
| `id` | string | Stable opportunity ID: `<CATEGORY>-<5 digits>`. The digits are deterministic for the same finding. |
| `category` | string | Category slug, e.g. `ALLOCATION`, `SIMD`, `PARALLELISM`, `DEPENDENCY` (spec §26). |
| `component` | string | Path or component the finding belongs to. |
| `impact` | integer | 1–10 (spec §28). |
| `confidence` | integer | Numeric scale `2`, `4`, `6`, `8`, `10` for Very Low … Very High (spec §29). |
| `effort` | integer | Numeric scale `1`, `3`, `5`, `7`, `9` for Trivial … Very High (spec §30). |
| `risk` | integer | Overall risk 1–10 (spec §31). |
| `generality` | integer | 1–10 (spec §32). |
| `priority` | integer or `null` | 0–100 when scored (spec §27). The formula is not a stable contract. |

Confidence and effort use the numeric scales above, so consumers can sort
them without a lookup table.

## Scan (`scan`)

```json
{ "schema_version": 1, "generated_by": "…", "data": { "name": "…", "projects": [ … ] } }
```

`data` is the discovered repository tree (repository → projects → crates).
Its field names are the model's serialised form and are not yet frozen; a
change to them will bump `schema_version`.

## Baselines (`.tpt-profiler/baselines/<name>.json`)

Baselines are stored as the same envelope, with the saved run as `data`.
They are the only persisted artefact in v0.1. A missing baseline is reported
as absent, while a corrupt one is an error (`DecodeFailed`) rather than being
skipped silently.
