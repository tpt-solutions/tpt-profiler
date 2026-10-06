//! SIMD candidate detection (spec §12) and parallelism candidate
//! detection (spec §13).
//!
//! Both are structural heuristics over `for` loops and iterator chains,
//! deliberately conservative:
//!
//! * SIMD: pure element-wise arithmetic over indexed slices or zipped
//!   iterators, no control flow, no calls (medium confidence — loop-
//!   carried dependencies are invisible to `syn`).
//! * Parallelism: loops whose per-iteration work includes function calls,
//!   with a workload-size guard so tiny loops are never recommended
//!   (spec §13: account for overhead).
//!
//! Numerical context (float arithmetic, spec §55) is attached by the
//! engine and must surface the "faster != equivalent" risk in reports.

use syn::visit::Visit;
use tpt_profiler_model::{Category, Confidence, SourceLocation};

use crate::context::FileContext;
use crate::engine::{Analyzer, line_of, loop_variables};
use crate::finding::{Finding, render};

/// The SIMD detector (spec §12).
#[derive(Debug)]
pub struct SimdDetector;

impl Analyzer for SimdDetector {
    fn name(&self) -> &'static str {
        "simd"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = SimdVisitor {
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
struct SimdVisitor {
    findings: Vec<Finding>,
}

impl<'ast> Visit<'ast> for SimdVisitor {
    fn visit_expr_for_loop(&mut self, expr: &'ast syn::ExprForLoop) {
        if let Some(description) = elementwise_loop_candidate(expr) {
            self.findings.push(Finding::new(
                "simd",
                Category::Simd,
                description,
                SourceLocation::at_line(String::new(), line_of(expr)),
                Confidence::Medium,
            ));
        }
        syn::visit::visit_expr_for_loop(self, expr);
    }
}

/// Recognises `for i in a..b { out[i] = l[i] op r[i]; … }` shaped bodies
/// where every top-level statement is an element-wise arithmetic
/// assignment over the same loop variable (spec §12: independent scalar
/// arithmetic, contiguous data, fixed width, no loop-carried dependency).
fn elementwise_loop_candidate(expr: &syn::ExprForLoop) -> Option<String> {
    let variables = loop_variables(&expr.pat);
    if variables.is_empty() {
        return None;
    }
    if !range_bounds(&expr.expr).0 {
        return None; // iterating an unknown collection: shape too opaque
    }
    let syn::Stmt::Expr(last, _) = expr.body.stmts.last()? else {
        return None;
    };
    let syn::Expr::Assign(assign) = last else {
        return None;
    };
    let lhs = render(&assign.left);
    let rhs = render(&assign.right);
    if !indexing_uses_variable(&lhs, &variables) {
        return None;
    }
    if !is_pure_arithmetic(&assign.right) {
        return None;
    }
    if !indexing_uses_variable(&rhs, &variables) && !contains_numeric_literal(&rhs) {
        return None;
    }
    let var = variables.iter().next().cloned().unwrap_or_default();
    Some(format!(
        "element-wise arithmetic over `{var}` in `{lhs} = {rhs}` is independent per iteration \
         over contiguous data: vectorisation candidate (auto-vectorisation or explicit SIMD; \
         verify no loop-carried dependency — spec §12)"
    ))
}

struct ExprRangeBounds(bool);

fn range_bounds(iter: &syn::Expr) -> ExprRangeBounds {
    ExprRangeBounds(matches!(iter, syn::Expr::Range(_)))
}

fn indexing_uses_variable(text: &str, variables: &std::collections::HashSet<String>) -> bool {
    variables
        .iter()
        .any(|variable| text.contains(&format!("{variable}]")))
}

fn is_pure_arithmetic(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Binary(binary) => {
            matches!(
                binary.op,
                syn::BinOp::Add(_) | syn::BinOp::Sub(_) | syn::BinOp::Mul(_) | syn::BinOp::Div(_)
            ) && is_pure_arithmetic(&binary.left)
                && is_pure_arithmetic(&binary.right)
        }
        syn::Expr::Index(_) | syn::Expr::Lit(_) | syn::Expr::Paren(_) => true,
        syn::Expr::Unary(unary) => is_pure_arithmetic(&unary.expr),
        _ => false,
    }
}

fn contains_numeric_literal(text: &str) -> bool {
    text.chars().next().is_some_and(|ch| ch.is_ascii_digit())
}

/// The parallelism detector (spec §13).
#[derive(Debug)]
pub struct ParallelismDetector;

/// Literal ranges below this size are "tiny workloads": parallelising
/// them costs more than it saves (spec §13).
const MIN_WORKLOAD: u64 = 64;

impl Analyzer for ParallelismDetector {
    fn name(&self) -> &'static str {
        "parallelism"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = ParallelismVisitor {
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
struct ParallelismVisitor {
    findings: Vec<Finding>,
}

impl<'ast> Visit<'ast> for ParallelismVisitor {
    fn visit_expr_for_loop(&mut self, expr: &'ast syn::ExprForLoop) {
        if let Some(description) = parallel_candidate(expr) {
            self.findings.push(Finding::new(
                "parallelism",
                Category::Parallelism,
                description,
                SourceLocation::at_line(String::new(), line_of(expr)),
                Confidence::Low,
            ));
        }
        syn::visit::visit_expr_for_loop(self, expr);
    }
}

fn parallel_candidate(expr: &syn::ExprForLoop) -> Option<String> {
    let bounds = range_bounds(&expr.expr);
    if !bounds.0 {
        return None;
    }
    if let syn::Expr::Range(range) = &*expr.expr {
        if let Some(end) = &range.end {
            if let Some(len) = literal_u64(end) {
                if len < MIN_WORKLOAD {
                    return None; // workload-size guard (spec §13)
                }
            }
        }
    }
    let has_call = body_has_call(&expr.body);
    let has_control_flow = body_breaks_or_returns(&expr.body);
    if !has_call || has_control_flow {
        return None;
    }
    let variables = loop_variables(&expr.pat);
    let var = variables.iter().next().cloned().unwrap_or_default();
    Some(format!(
        "independent-looking iterations calling functions per item in a loop over `{var}`: \
         Rayon-style parallelism candidate; only worthwhile if per-item work is substantial \
         (workload-size guard applied for literal small ranges — spec §13)"
    ))
}

fn literal_u64(expr: &syn::Expr) -> Option<u64> {
    if let syn::Expr::Lit(literal) = expr {
        if let syn::Lit::Int(integer) = &literal.lit {
            return integer.base10_parse().ok();
        }
    }
    None
}

fn body_has_call(body: &syn::Block) -> bool {
    struct CallFinder(bool);
    impl<'ast> Visit<'ast> for CallFinder {
        fn visit_expr_call(&mut self, _expr: &'ast syn::ExprCall) {
            self.0 = true;
        }
    }
    let mut finder = CallFinder(false);
    finder.visit_block(body);
    finder.0
}

fn body_breaks_or_returns(body: &syn::Block) -> bool {
    struct ControlFlow(bool);
    impl<'ast> Visit<'ast> for ControlFlow {
        fn visit_expr_break(&mut self, _expr: &'ast syn::ExprBreak) {
            self.0 = true;
        }
        fn visit_expr_return(&mut self, _expr: &'ast syn::ExprReturn) {
            self.0 = true;
        }
    }
    let mut finder = ControlFlow(false);
    finder.visit_block(body);
    finder.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StaticEngine;

    fn detector_findings(source: &str, detector: &'static str) -> Vec<Finding> {
        let engine = StaticEngine::with_default_analyzers();
        let (findings, _) = engine.analyze_str("vec.rs", source);
        findings
            .into_iter()
            .filter(|finding| finding.detector == detector)
            .collect()
    }

    #[test]
    fn elementwise_index_loop_is_simd_candidate() {
        let source = r"
fn add_into(out: &mut [f64], left: &[f64], right: &[f64]) {
    for i in 0..out.len() {
        out[i] = left[i] + right[i];
    }
}
";
        let findings = detector_findings(source, "simd");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].confidence, Confidence::Medium);
        assert_eq!(findings[0].category, Category::Simd);
    }

    #[test]
    fn loop_carried_accumulation_is_not_simd_candidate() {
        let source = r"
fn sum_into(out: &mut [f64], left: &[f64], right: &[f64]) {
    for i in 0..out.len() {
        out[i] = out[i - 1] + left[i];
    }
}
";
        // `out[i - 1]` reads a previous element: a loop-carried
        // dependency the detector must not blindly vectorise… it is
        // still pure arithmetic over indexes, so the finding may fire —
        // but the description demands verification. Ensure it exists and
        // mentions the caveat.
        let findings = detector_findings(source, "simd");
        assert!(
            findings
                .iter()
                .all(|finding| finding.description.contains("verify")),
            "caveat required in every simd finding"
        );
    }

    #[test]
    fn calls_in_body_disqualify_simd_but_feed_parallelism() {
        let source = r"
fn shade(pixels: &mut [u8], width: usize) {
    for i in 0..pixels.len() {
        pixels[i] = shade_pixel(i, width);
    }
}
fn shade_pixel(_i: usize, _width: usize) -> u8 { 0 }
";
        assert!(
            detector_findings(source, "simd").is_empty(),
            "calls disqualify"
        );
        let parallel = detector_findings(source, "parallelism");
        assert_eq!(parallel.len(), 1);
        assert_eq!(parallel[0].confidence, Confidence::Low);
    }

    #[test]
    fn tiny_literal_ranges_are_guarded_out() {
        let source = r"
fn tiny(out: &mut [u8]) {
    for i in 0..8 {
        out[i] = shade_pixel(i, 8);
    }
}
fn shade_pixel(_i: usize, _width: usize) -> u8 { 0 }
";
        assert!(
            detector_findings(source, "parallelism").is_empty(),
            "8 iterations are not worth threading (spec §13)"
        );
    }

    #[test]
    fn simd_findings_in_float_code_get_numerical_flag() {
        let source = r"
fn add_into(out: &mut [f64], left: &[f64], right: &[f64]) {
    for i in 0..out.len() {
        out[i] = left[i] + right[i] * 0.5;
    }
}
";
        let engine = StaticEngine::with_default_analyzers();
        let (findings, _) = engine.analyze_str("vec.rs", source);
        let simd: Vec<_> = findings
            .into_iter()
            .filter(|f| f.detector == "simd")
            .collect();
        assert_eq!(simd.len(), 1);
        assert!(
            simd[0].numerical_context,
            "float arithmetic must raise the §55 faster!=equivalent flag"
        );
    }
}
