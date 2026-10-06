//! Obvious loop opportunities (spec §9): loop-invariant computation that
//! can be hoisted.
//!
//! A `let` binding inside a loop whose initialiser calls a function with
//! arguments that do not mention the loop variable is a hoisting
//! candidate. Confidence is `Low`: the call may be cheap or intentionally
//! re-evaluated (e.g. `Instant::now()`).

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
/// arguments never mention the loop variable. Descends into nested
/// blocks; nested loops are re-checked with their own variables when the
/// outer visitor reaches them.
struct InvariantCallFinder<'a, 'ast> {
    variables: &'a std::collections::HashSet<String>,
    invariants: Vec<&'ast syn::Expr>,
}

impl<'ast> Visit<'ast> for InvariantCallFinder<'_, 'ast> {
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if let Some(init) = local.init.as_ref().map(|boxed| boxed.expr.as_ref()) {
            // Free functions and method calls both count; the loop
            // variable must not appear anywhere in the expression.
            let (callee_text, args_text) = match init {
                syn::Expr::Call(call) => (render(&call.func), render(call)),
                syn::Expr::MethodCall(method) => (render(&method.receiver), render(method)),
                _ => {
                    syn::visit::visit_local(self, local);
                    return;
                }
            };
            let uses_loop_var = self
                .variables
                .iter()
                .any(|variable| args_text.contains(variable));
            // `Instant::now()` and friends are either intentionally
            // per-iteration or cheap; skip the obvious ones.
            if !uses_loop_var && !callee_text.contains("now") {
                self.invariants.push(init);
            }
        }
        syn::visit::visit_local(self, local);
    }
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
}
