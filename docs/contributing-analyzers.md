# Adding a static analyzer

Audience: contributors who want the profiler to notice a new kind of
pattern in Rust source (spec §9–17, §67).

A static analyzer is a small `syn` visitor. It reads one parsed file and
returns findings. It never runs the project's code, never reads types, and
never modifies source (spec §53, §72). Everything it reports is a *static
pattern*, so it should never claim more than `Medium` confidence (spec §29).
The opportunity engine enforces that at the report level: a static pattern
never produces an opportunity above `Medium`, whatever the detector says. The
detector's own value is still shown on the finding, so keep it honest.

## Where things live

| Piece | Location |
|---|---|
| Analyzer trait | `crates/tpt-profiler-static/src/engine.rs` (`Analyzer`) |
| Registry of built-in analyzers | `StaticEngine::with_default_analyzers` in `engine.rs` |
| Detector modules | `crates/tpt-profiler-static/src/detectors/` |
| Shared loop helper | `LoopBindings` in `detectors/mod.rs` |
| Finding type | `crates/tpt-profiler-static/src/finding.rs` (`Finding`) |
| Regression corpus | `crates/tpt-profiler-static/tests/corpus/` and `tests/regression.rs` |

## Steps

1. **Pick a category.** Use an existing `Category` from
   `tpt-profiler-model` (spec §26). Add a new one only with a spec change.

2. **Create the module.** Add `src/detectors/<name>.rs` and declare it in
   `detectors/mod.rs` with `pub mod <name>;`.

3. **Implement `Analyzer`.** The module needs a unit struct, an `Analyzer`
   impl, and a `syn::visit::Visit` visitor that does the work:

   ```rust
   use syn::visit::Visit;
   use tpt_profiler_model::{Category, Confidence, SourceLocation};

   use crate::context::FileContext;
   use crate::detectors::LoopBindings;
   use crate::engine::Analyzer;
   use crate::finding::Finding;

   pub struct ExampleDetector;

   impl Analyzer for ExampleDetector {
       fn name(&self) -> &'static str {
           "example"
       }

       fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
           let mut visitor = ExampleVisitor { findings: Vec::new() };
           visitor.visit_file(file);
           for finding in &mut visitor.findings {
               finding.location.file.clone_from(&context.file);
           }
           visitor.findings
       }
   }

   struct ExampleVisitor {
       findings: Vec<Finding>,
   }

   impl<'ast> Visit<'ast> for ExampleVisitor {
       // Override visit_expr_* methods here and push findings.
   }
   ```

   Two rules to keep:

   * The detector string passed to `Finding::new` must equal the value
     returned by `name()`. The existing detectors repeat the literal in both
     places, and nothing checks that they match. Copy the literal exactly.
   * Set the file path on each finding, as above. The line is set by
     `SourceLocation::at_line`.

4. **Choose confidence honestly.** Stay at or below `Confidence::Medium` for
   static patterns. The opportunity engine caps what is reported, but the
   detector's value is its own claim, so do not rely on the cap. Use `Low` when the pattern is weak or the receiver type is
   a guess (the existing detectors do this for `Vec::remove(0)` and
   `write_all`). A finding with no evidence is a bug, not a low-confidence
   finding (spec §9).

5. **Track loops when the rule depends on them.** For "repeated work"
   patterns, call `LoopBindings::enter` and `leave` around loop bodies, and
   use `depends_on_loop` to flag only work on loop-invariant input. Names
   bound inside the loop count as varying, so the check errs toward silence.

6. **Register it.** Add a `engine.register(Box::new(...))` line in
   `StaticEngine::with_default_analyzers`. Registration order does not change
   output: findings are sorted deterministically (spec §75).

7. **Write unit tests.** Each detector has positive and negative tests
   against `StaticEngine::analyze_str` or the analyzer directly. Cover the
   pattern, the same code outside a loop, and the near-miss that must stay
   quiet. Look at `detectors/io.rs` for the layout.

8. **Add corpus expectations.** In `tests/corpus/hot_loop.rs`, put a
   `// expect: <detector>` marker on each line that must fire. The regression
   test fails if an expected finding is missing. `tests/corpus/clean.rs` must
   produce zero findings, so add a line there that the detector must *not*
   flag. This is the guard against false positives (spec §76).

9. **Run the gates.**

   ```text
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

## Analyzers outside the built-in set

The analyzer seam (spec §67) is `Analyzer` plus `StaticEngine::register`.
A caller that builds its own engine runs the built-in detectors and its own
together through `pipeline::analyse_with`. The complete, runnable example is
`crates/tpt-profiler-static/examples/custom_analyzer.rs`: it flags case
conversions inside loops and is about 100 lines including its sample.

```text
cargo run -p tpt-profiler-static --example custom_analyzer
```

Loading analyzers at run time from outside the crate (dynamic libraries or
separate executables) is not supported yet. Analyzers are compiled in.

## Suppression

Users can silence a finding with `// tpt-profiler:ignore` on the same line, or
`// tpt-profiler:ignore-next` on the line above. The engine applies this
after analysis, so detectors need no suppression logic.

## Unsafe and numerical context

The engine tags findings inside `unsafe` blocks and functions, and sets
`numerical_context` for code with floating-point literals, casts, or
transcendental calls (spec §54, §55). Do not duplicate that logic in a
detector. The opportunity engine uses these flags to raise effort and risk.

## What the analyzer must not do

* Run the project, its build scripts, or its tests.
* Use the network.
* Depend on iteration order of hash maps or on wall-clock time.
* Panic on malformed input. Parse failures are reported by the engine; an
  analyzer only ever sees a `syn::File` that parsed.
