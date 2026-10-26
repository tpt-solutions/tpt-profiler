//! GPU-suitability indicators (spec §22, Phase 4.2), from source alone.
//!
//! A loop nest is a *candidate* when it runs over a large literal range,
//! its body is straight-line arithmetic on indexed data, and it contains no
//! branches or calls. Branches diverge across GPU lanes, and calls are
//! usually not device-side. These are indicators only: the finding asks for
//! runtime evidence before any offload is considered (spec §22's
//! strong-evidence threshold), so it is always Low confidence.

use syn::visit::Visit;
use tpt_profiler_model::{Category, Confidence, SourceLocation};

use crate::context::FileContext;
use crate::engine::{Analyzer, line_of};
use crate::finding::Finding;

/// Smallest literal range end that counts as a large workload.
const LARGE_RANGE: u64 = 1024;

/// Flags large, branch-free, arithmetic-only loop nests.
#[derive(Debug)]
pub struct GpuCandidateDetector;

impl Analyzer for GpuCandidateDetector {
    fn name(&self) -> &'static str {
        "gpu_candidate"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = GpuVisitor {
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
struct GpuVisitor {
    findings: Vec<Finding>,
}

impl<'ast> Visit<'ast> for GpuVisitor {
    fn visit_expr_for_loop(&mut self, expr: &'ast syn::ExprForLoop) {
        if large_literal_range(&expr.expr) && is_offload_shaped(&expr.body) {
            self.findings.push(Finding::new(
                "gpu_candidate",
                Category::Gpu,
                String::from(
                    "large branch-free arithmetic loop over indexed data is a GPU-offload \
                     indicator; confirm with runtime measurements before considering a device \
                     kernel (spec §22)",
                ),
                SourceLocation::at_line(String::new(), line_of(expr)),
                Confidence::Low,
            ));
        }
        syn::visit::visit_expr_for_loop(self, expr);
    }
}

/// Whether the iterator is a range whose literal end is at least
/// [`LARGE_RANGE`], such as `0..4096` or `0..n` is *not* (no literal).
fn large_literal_range(iterator: &syn::Expr) -> bool {
    let syn::Expr::Range(range) = iterator else {
        return false;
    };
    let Some(end) = range.end.as_deref() else {
        return false;
    };
    matches!(
        end,
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(literal),
            ..
        }) if literal.base10_parse::<u64>().is_ok_and(|value| value >= LARGE_RANGE)
    )
}

/// Whether a loop body is straight-line arithmetic over indexed data: it
/// has no branches, no calls, and at least one indexed operand.
fn is_offload_shaped(body: &syn::Block) -> bool {
    #[derive(Default)]
    struct Shape {
        branches_or_calls: bool,
        indexes: bool,
    }
    impl<'ast> Visit<'ast> for Shape {
        fn visit_expr_if(&mut self, _: &'ast syn::ExprIf) {
            self.branches_or_calls = true;
        }
        fn visit_expr_match(&mut self, _: &'ast syn::ExprMatch) {
            self.branches_or_calls = true;
        }
        fn visit_expr_call(&mut self, _: &'ast syn::ExprCall) {
            self.branches_or_calls = true;
        }
        fn visit_expr_method_call(&mut self, _: &'ast syn::ExprMethodCall) {
            self.branches_or_calls = true;
        }
        fn visit_expr_index(&mut self, index: &'ast syn::ExprIndex) {
            self.indexes = true;
            syn::visit::visit_expr_index(self, index);
        }
    }
    let mut shape = Shape::default();
    shape.visit_block(body);
    !shape.branches_or_calls && shape.indexes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StaticEngine;

    fn gpu_findings(source: &str) -> Vec<Finding> {
        let engine = StaticEngine::with_default_analyzers();
        let (findings, _) = engine.analyze_str("gpu.rs", source);
        findings
            .into_iter()
            .filter(|finding| finding.detector == "gpu_candidate")
            .collect()
    }

    #[test]
    fn large_arithmetic_loop_over_indexed_data_is_a_candidate() {
        let source = r"
fn scale(data: &mut [f32]) {
    for i in 0..4096 {
        data[i] = data[i] * 2.0 + 1.0;
    }
}
";
        let findings = gpu_findings(source);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].category, Category::Gpu);
    }

    #[test]
    fn branching_body_is_not_a_candidate_because_lanes_diverge() {
        let source = r"
fn clamp(data: &mut [f32]) {
    for i in 0..4096 {
        if data[i] < 0.0 { data[i] = 0.0; }
    }
}
";
        assert!(gpu_findings(source).is_empty());
    }

    #[test]
    fn small_or_unknown_ranges_are_not_candidates() {
        let source = r"
fn scale(data: &mut [f32], n: usize) {
    for i in 0..8 {
        data[i] = data[i] * 2.0;
    }
    for i in 0..n {
        data[i] = data[i] * 2.0;
    }
}
";
        assert!(gpu_findings(source).is_empty());
    }
}
