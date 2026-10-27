//! Clone analysis (spec §10).
//!
//! Distinguishes cheap reference-count clones from possible deep copies
//! *heuristically*: without type information, `Arc::clone(&x)` path calls
//! are recognised exactly, but `x.clone()` where `x` looks like an
//! `Arc`/`Rc` binding is a name-based guess. This limit is inherent to
//! `syn`-level analysis and is why findings stay at `Medium` confidence.
//!
//! Note the asymmetry with the spec: `Arc::clone` in a loop is *fine*
//! (it is an atomic increment), so it is never a finding; the finding is
//! the possible **deep** copy per iteration.

use syn::visit::Visit;
use tpt_profiler_model::{Category, Confidence, SourceLocation};

use crate::context::FileContext;
use crate::engine::{Analyzer, line_of};
use crate::finding::{Finding, render};

/// Flags `.clone()` calls inside loops that are likely deep copies.
#[derive(Debug)]
pub struct CloneDetector;

impl Analyzer for CloneDetector {
    fn name(&self) -> &'static str {
        "clone"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = CloneVisitor {
            depth: 0,
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
struct CloneVisitor {
    depth: u32,
    findings: Vec<Finding>,
}

impl<'ast> Visit<'ast> for CloneVisitor {
    crate::engine::impl_loop_depth!();

    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        if self.depth > 0 && expr.method == "clone" && !looks_like_reference_counted(&expr.receiver)
        {
            let line = line_of(expr);
            self.findings.push(Finding::new(
                "clone",
                Category::Allocation,
                format!(
                    "`.clone()` on `{}` inside a loop may deep-copy the value every iteration; \
                     if the value is large, borrow, index, or share via `Arc` instead (spec §10)",
                    render(&expr.receiver)
                ),
                SourceLocation::at_line(String::new(), line),
                Confidence::Medium,
            ));
        }
        syn::visit::visit_expr_method_call(self, expr);
    }
}

/// Name-based heuristic for `arc_config.clone()` / `rc_counter.clone()`
/// receivers; documents the `syn` type-info limit (spec §10).
fn looks_like_reference_counted(receiver: &syn::Expr) -> bool {
    let lowercase = render(receiver).to_lowercase();
    lowercase.starts_with("arc") || lowercase.starts_with("rc")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StaticEngine;

    fn findings_of(source: &str) -> Vec<Finding> {
        let engine = StaticEngine::with_default_analyzers();
        let (findings, _) = engine.analyze_str("clone.rs", source);
        findings
            .into_iter()
            .filter(|finding| finding.detector == "clone")
            .collect()
    }

    #[test]
    fn clone_in_loop_is_flagged() {
        let source = r"
fn process(items: &[BigStruct], cache: &mut Vec<BigStruct>) {
    for item in items {
        cache.push(item.clone());
    }
}
";
        let findings = findings_of(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].location.line_start, 4);
        assert_eq!(findings[0].location.file, "clone.rs");
        assert_eq!(findings[0].category, Category::Allocation);
        assert_eq!(findings[0].confidence, Confidence::Medium);
    }

    #[test]
    fn arc_clone_path_call_is_not_a_finding() {
        let source = r"
use std::sync::Arc;
fn share(data: Arc<Config>, slots: &mut Vec<Arc<Config>>) {
    for _ in 0..1000 {
        slots.push(Arc::clone(&data));
    }
}
";
        assert!(
            findings_of(source).is_empty(),
            "Arc::clone is an atomic increment"
        );
    }

    #[test]
    fn clone_outside_loop_is_not_a_finding() {
        let source = r"
fn once(value: &BigStruct) -> BigStruct {
    value.clone()
}
";
        assert_eq!(findings_of(source).len(), 0);
    }

    #[test]
    fn receiver_named_arc_is_skipped_by_heuristic() {
        let source = r"
fn share(arc_config: &std::sync::Arc<Config>, slots: &mut Vec<std::sync::Arc<Config>>) {
    for _ in 0..100 {
        slots.push(arc_config.clone());
    }
}
";
        assert!(
            findings_of(source).is_empty(),
            "heuristic: `arc_*` receiver"
        );
    }
}
