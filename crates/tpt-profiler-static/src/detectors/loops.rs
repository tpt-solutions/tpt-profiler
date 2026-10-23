//! Obvious loop opportunities (spec §9): loop-invariant computation that
//! can be hoisted.
//!
//! A `let` binding inside a loop whose initialiser calls a function with
//! arguments that do not depend on the loop variable (or anything else
//! bound per iteration) is a hoisting candidate. Confidence is `Low`: the call may be cheap or intentionally
//! re-evaluated (e.g. `Instant::now()`).

use std::collections::HashSet;

use syn::visit::Visit;
use tpt_profiler_model::{Category, Confidence, SourceLocation};

use crate::context::FileContext;
use crate::engine::{Analyzer, line_of, loop_variables};
use crate::finding::{Finding, render};

/// Flags loop-invariant calls (spec §9).
#[derive(Debug)]
pub struct LoopHoistingDetector;

impl Analyzer for LoopHoistingDetector {
    fn name(&self) -> &'static str {
        "loops"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = HoistingVisitor {
            findings: Vec::new(),
        };
        visitor.visit_file(file);
        for finding in &mut visitor.findings {
            finding.location.file.clone_from(&context.file);
        }
        visitor.findings
    }
}

#[derive(Debug)]
struct HoistingVisitor {
    findings: Vec<Finding>,
}

impl<'ast> Visit<'ast> for HoistingVisitor {
    fn visit_expr_for_loop(&mut self, expr: &'ast syn::ExprForLoop) {
        let variables = loop_variables(&expr.pat);
        let mut finder = InvariantCallFinder {
            variables: &variables,
            body_bindings: HashSet::new(),
            invariants: Vec::new(),
        };
        finder.visit_block(&expr.body);
        for init in finder.invariants {
            let line = line_of(init);
            self.findings.push(Finding::new(
                "loops",
                Category::Algorithm,
                format!(
                    "`{}` does not depend on the loop variable; \
                     if the call is expensive, hoist it out of the loop (spec §9)",
                    render(init)
                ),
                SourceLocation::at_line(String::new(), line),
                Confidence::Low,
            ));
        }
        syn::visit::visit_expr_for_loop(self, expr);
    }
}

/// Collects `let x = call(..)` bindings anywhere in the loop body whose
/// arguments depend on nothing that varies per iteration. "Varies" means
/// the loop's own variables, names bound by earlier statements in the body,
/// and closure parameters inside the initialiser. Descends into nested
/// blocks; nested loops are re-checked with their own variables when the
/// outer visitor reaches them.
struct InvariantCallFinder<'a, 'ast> {
    variables: &'a HashSet<String>,
    body_bindings: HashSet<String>,
    invariants: Vec<&'ast syn::Expr>,
}

impl<'ast> Visit<'ast> for InvariantCallFinder<'_, 'ast> {
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let Some(init) = local.init.as_ref().map(|boxed| boxed.expr.as_ref()) {
            if let Some((callee_text, args_text)) = call_texts(init) {
                let mut varying: HashSet<String> = self
                    .variables
                    .iter()
                    .chain(&self.body_bindings)
                    .cloned()
                    .collect();
                varying.extend(closure_parameters(init));
                let uses_varying = mentions_any(&args_text, &varying);
                // `Instant::now()` and friends are either intentionally
                // per-iteration or cheap; skip the obvious ones.
                if !uses_varying && !callee_text.contains("now") {
                    self.invariants.push(init);
                }
            }
        }
        self.body_bindings.extend(loop_variables(&local.pat));
        syn::visit::visit_local(self, local);
    }
}

/// Rendered callee and full text of a free-function or method call; `None`
/// for any other initialiser.
fn call_texts(init: &syn::Expr) -> Option<(String, String)> {
    match init {
        syn::Expr::Call(call) => Some((render(&call.func), render(call))),
        syn::Expr::MethodCall(method) => Some((render(&method.receiver), render(method))),
        _ => None,
    }
}

/// Whether `text` contains any of `names` as a whole identifier (so `i`
/// does not match `items`).
fn mentions_any(text: &str, names: &HashSet<String>) -> bool {
    text.split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .any(|token| names.contains(token))
}

/// Names bound by closure parameters anywhere within `expr`.
fn closure_parameters(expr: &syn::Expr) -> HashSet<String> {
    struct Closures(HashSet<String>);
    impl<'ast> Visit<'ast> for Closures {
        fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
            for input in &closure.inputs {
                self.0.extend(loop_variables(input));
            }
            syn::visit::visit_expr_closure(self, closure);
        }
    }
    let mut visitor = Closures(HashSet::new());
    visitor.visit_expr(expr);
    visitor.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StaticEngine;

    fn findings_of(source: &str) -> Vec<Finding> {
        let engine = StaticEngine::with_default_analyzers();
        let (findings, _) = engine.analyze_str("loops.rs", source);
        findings
            .into_iter()
            .filter(|finding| finding.detector == "loops")
            .collect()
    }

    #[test]
    fn invariant_call_is_flagged() {
        let source = r"
fn render(items: &[u32], palette: &Palette) -> Vec<u32> {
    let mut out = Vec::new();
    for item in items {
        let colour = palette.computed_colour();
        out.push(item.wrapping_mul(colour));
    }
    out
}
struct Palette;
impl Palette { fn computed_colour(&self) -> u32 { 7 } }
";
        let findings = findings_of(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].location.line_start, 5);
        assert_eq!(findings[0].category, Category::Algorithm);
    }

    #[test]
    fn dependent_call_is_not_flagged() {
        let source = r"
fn render(items: &[u32], palette: &Palette) -> Vec<u32> {
    let mut out = Vec::new();
    for item in items {
        let colour = palette.for_item(item);
        out.push(colour);
    }
    out
}
struct Palette;
impl Palette { fn for_item(&self, _item: &u32) -> u32 { 7 } }
";
        assert!(findings_of(source).is_empty());
    }

    #[test]
    fn instant_now_is_not_flagged() {
        let source = r"
fn tick(rounds: u32) {
    for _ in 0..rounds {
        let started = std::time::Instant::now();
        std::hint::black_box(started);
    }
}
";
        assert!(findings_of(source).is_empty());
    }

    #[test]
    fn call_on_a_body_local_is_not_flagged() {
        // `resolved` depends on `view`, which is rebuilt every iteration;
        // only `make_view()` itself is invariant.
        let source = r"
fn run(names: &Names, n: usize) -> usize {
    let mut total = 0;
    for _ in 0..n {
        let view = make_view();
        let resolved = names.resolve(&view);
        total += resolved;
    }
    total
}
";
        let findings = findings_of(source);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].description.contains("make_view"));
    }

    #[test]
    fn closure_parameters_count_as_per_iteration_values() {
        let source = r"
fn run(items: &[u32], factor: u32, n: usize) -> usize {
    let mut total = 0;
    for _ in 0..n {
        let scaled: Vec<u32> = items.iter().map(|item| scale(item, factor)).collect();
        total += scaled.len();
    }
    total
}
";
        assert!(findings_of(source).is_empty());
    }

    #[test]
    fn loop_variable_matches_whole_identifiers_only() {
        // `items` contains the letter `i`, but it is not the loop variable.
        let source = r"
fn run(items: &[u32], n: usize) -> usize {
    let mut total = 0;
    for i in 0..n {
        let built = build(items);
        total += built.len() + i;
    }
    total
}
";
        assert_eq!(findings_of(source).len(), 1);
    }
}
