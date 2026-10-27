//! Algorithmic complexity heuristics (spec §2): patterns whose cost grows
//! faster than the input size, detected without type information.
//!
//! * Nested loops over the same collection do quadratic work.
//! * `remove(0)` or `insert(0, …)` inside a loop shifts every element on
//!   each iteration.
//! * Sorting inside a loop repeats an `O(n log n)` pass per iteration.
//!
//! The nested-loop check is `Low` confidence: pairwise comparison over one
//! collection is sometimes exactly what is wanted. The other two are
//! `Medium`, since the repeated work is almost never intended.

use syn::visit::Visit;
use tpt_profiler_model::{Category, Confidence, SourceLocation};

use crate::context::FileContext;
use crate::engine::{Analyzer, line_of};
use crate::finding::{Finding, render};

/// Flags super-linear patterns (spec §2).
#[derive(Debug)]
pub struct AlgorithmDetector;

impl Analyzer for AlgorithmDetector {
    fn name(&self) -> &'static str {
        "algorithm"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = AlgorithmVisitor {
            loops: Vec::new(),
            findings: Vec::new(),
        };
        visitor.visit_file(file);
        for finding in &mut visitor.findings {
            finding.location.file.clone_from(&context.file);
        }
        visitor.findings
    }
}

/// Method names that sort their receiver.
const SORTS: &[&str] = &[
    "sort",
    "sort_unstable",
    "sort_by",
    "sort_by_key",
    "sort_unstable_by",
    "sort_unstable_by_key",
];

#[derive(Debug)]
struct AlgorithmVisitor {
    /// Normalised iterable of each enclosing `for` loop, outermost first.
    loops: Vec<String>,
    findings: Vec<Finding>,
}

impl AlgorithmVisitor {
    fn push(&mut self, line: u32, confidence: Confidence, description: String) {
        self.findings.push(Finding::new(
            "algorithm",
            Category::Algorithm,
            description,
            SourceLocation::at_line(String::new(), line),
            confidence,
        ));
    }

    fn in_loop(&self) -> bool {
        !self.loops.is_empty()
    }
}

/// The collection a loop iterates over, with borrows and iterator adaptors
/// removed, so `&xs`, `xs.iter()` and `xs` compare equal.
fn iterable_key(iterable: &syn::Expr) -> String {
    let inner = match iterable {
        syn::Expr::Reference(reference) => &*reference.expr,
        other => other,
    };
    let mut text = render(inner);
    for suffix in [".iter()", ".iter_mut()", ".into_iter()"] {
        if let Some(stripped) = text.strip_suffix(suffix) {
            text = stripped.to_owned();
        }
    }
    text
}

impl<'ast> Visit<'ast> for AlgorithmVisitor {
    fn visit_expr_for_loop(&mut self, expr: &'ast syn::ExprForLoop) {
        let key = iterable_key(&expr.expr);
        if self.loops.contains(&key) {
            self.push(
                line_of(expr),
                Confidence::Low,
                format!(
                    "nested loops both iterate `{key}`, so the inner body runs O(n²) \
                     times; check whether a lookup table, a sort, or a single pass \
                     would do (spec §2)"
                ),
            );
        }
        self.loops.push(key);
        syn::visit::visit_expr_for_loop(self, expr);
        self.loops.pop();
    }

    fn visit_expr_while(&mut self, expr: &'ast syn::ExprWhile) {
        self.loops.push(String::from("while"));
        syn::visit::visit_expr_while(self, expr);
        self.loops.pop();
    }

    fn visit_expr_loop(&mut self, expr: &'ast syn::ExprLoop) {
        self.loops.push(String::from("loop"));
        syn::visit::visit_expr_loop(self, expr);
        self.loops.pop();
    }

    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        if self.in_loop() {
            let name = expr.method.to_string();
            let line = line_of(expr);
            if (name == "remove" || name == "insert") && starts_at_front(expr) {
                // `HashMap::insert(0, …)` looks the same without types, so
                // this stays at Low confidence.
                self.push(
                    line,
                    Confidence::Low,
                    format!(
                        "`{}` shifts every element if the receiver is a `Vec`, which makes \
                         the loop O(n²); use a `VecDeque`, or iterate in reverse (spec §2)",
                        render(expr)
                    ),
                );
            } else if SORTS.contains(&name.as_str()) {
                self.push(
                    line,
                    Confidence::Medium,
                    format!(
                        "`{}` sorts on every loop iteration (O(n log n) each); sort once \
                         outside the loop, or keep the order incrementally (spec §2)",
                        render(expr)
                    ),
                );
            }
        }
        syn::visit::visit_expr_method_call(self, expr);
    }
}

/// Whether a `remove`/`insert` call targets index 0: `remove(0)` or
/// `insert(0, value)`.
fn starts_at_front(expr: &syn::ExprMethodCall) -> bool {
    let first = expr.args.first();
    matches!(
        first,
        Some(syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(literal),
            ..
        })) if literal.base10_digits() == "0"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StaticEngine;

    fn algorithm_findings(source: &str) -> Vec<Finding> {
        let engine = StaticEngine::with_default_analyzers();
        let (findings, _) = engine.analyze_str("algo.rs", source);
        findings
            .into_iter()
            .filter(|finding| finding.detector == "algorithm")
            .collect()
    }

    #[test]
    fn nested_loops_over_the_same_collection_are_flagged() {
        let source = r"
fn pairs(values: &[u32]) -> u32 {
    let mut total = 0;
    for a in values {
        for b in values.iter() {
            total += a * b;
        }
    }
    total
}
";
        let findings = algorithm_findings(source);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].location.line_start, 5);
        assert_eq!(findings[0].confidence, Confidence::Low);
    }

    #[test]
    fn nested_loops_over_different_collections_are_not_flagged() {
        let source = r"
fn cross(left: &[u32], right: &[u32]) -> u32 {
    let mut total = 0;
    for a in left {
        for b in right {
            total += a * b;
        }
    }
    total
}
";
        assert_eq!(algorithm_findings(source).len(), 0);
    }

    #[test]
    fn remove_at_front_inside_a_loop_is_flagged() {
        let source = r"
fn drain(mut queue: Vec<u32>) -> u32 {
    let mut total = 0;
    while !queue.is_empty() {
        total += queue.remove(0);
    }
    total
}
";
        let findings = algorithm_findings(source);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].confidence, Confidence::Low);
    }

    #[test]
    fn insert_at_front_inside_a_loop_is_flagged() {
        let source = r"
fn prepend(items: &[u32]) -> Vec<u32> {
    let mut out = Vec::new();
    for item in items {
        out.insert(0, *item);
    }
    out
}
";
        assert_eq!(algorithm_findings(source).len(), 1);
    }

    #[test]
    fn remove_last_inside_a_loop_is_not_flagged() {
        let source = r"
fn pop_all(mut stack: Vec<u32>) {
    while !stack.is_empty() {
        stack.remove(stack.len() - 1);
    }
}
";
        assert_eq!(algorithm_findings(source).len(), 0);
    }

    #[test]
    fn sort_inside_a_loop_is_flagged() {
        let source = r"
fn rounds(mut values: Vec<u32>, rounds: u32) -> Vec<u32> {
    for _ in 0..rounds {
        values.sort_unstable();
    }
    values
}
";
        let findings = algorithm_findings(source);
        assert_eq!(findings.len(), 1, "{findings:?}");
    }

    #[test]
    fn sort_outside_a_loop_is_not_flagged() {
        let source = r"
fn once(mut values: Vec<u32>) -> Vec<u32> {
    values.sort();
    values
}
";
        assert_eq!(algorithm_findings(source).len(), 0);
    }
}
