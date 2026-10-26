//! Lock detection (spec §57, Phase 4.5): taking the same lock on every loop
//! iteration, where one lock around the whole loop, or a batch of work
//! under one guard, would do.
//!
//! Locking a different lock per item (`slots[i].lock()`) is expected work
//! and is not flagged. Receivers are matched by name, so a lock behind a
//! function call is not seen; that limit is stated in the finding text.

use syn::visit::Visit;
use tpt_profiler_model::{Category, Confidence, SourceLocation};

use crate::context::FileContext;
use crate::engine::{Analyzer, line_of};
use crate::finding::{Finding, render};

use super::LoopBindings;

/// Flags `.lock()` on a loop-invariant receiver inside a loop.
#[derive(Debug)]
pub struct LockInLoopDetector;

impl Analyzer for LockInLoopDetector {
    fn name(&self) -> &'static str {
        "lock_in_loop"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = LockVisitor {
            bindings: LoopBindings::default(),
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
struct LockVisitor {
    bindings: LoopBindings,
    findings: Vec<Finding>,
}

impl<'ast> Visit<'ast> for LockVisitor {
    fn visit_expr_for_loop(&mut self, expr: &'ast syn::ExprForLoop) {
        self.bindings.enter(Some(&expr.pat), &expr.body);
        syn::visit::visit_expr_for_loop(self, expr);
        self.bindings.leave();
    }

    fn visit_expr_while(&mut self, expr: &'ast syn::ExprWhile) {
        self.bindings.enter(None, &expr.body);
        syn::visit::visit_expr_while(self, expr);
        self.bindings.leave();
    }

    fn visit_expr_loop(&mut self, expr: &'ast syn::ExprLoop) {
        self.bindings.enter(None, &expr.body);
        syn::visit::visit_expr_loop(self, expr);
        self.bindings.leave();
    }

    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        if self.bindings.in_loop() && expr.method == "lock" && expr.args.is_empty() {
            let receiver: &syn::Expr = &expr.receiver;
            if !self.bindings.depends_on_loop(&[receiver]) {
                self.findings.push(Finding::new(
                    "lock_in_loop",
                    Category::Concurrency,
                    format!(
                        "`{}.lock()` takes the same lock on every loop iteration; lock once \
                         outside the loop or batch the work under one guard (spec §57)",
                        render(&expr.receiver)
                    ),
                    SourceLocation::at_line(String::new(), line_of(expr)),
                    Confidence::Medium,
                ));
            }
        }
        syn::visit::visit_expr_method_call(self, expr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StaticEngine;

    fn lock_findings(source: &str) -> Vec<Finding> {
        let engine = StaticEngine::with_default_analyzers();
        let (findings, _) = engine.analyze_str("lock.rs", source);
        findings
            .into_iter()
            .filter(|finding| finding.detector == "lock_in_loop")
            .collect()
    }

    #[test]
    fn shared_lock_taken_per_iteration_is_flagged() {
        let source = r"
use std::sync::Mutex;
fn collect(items: &[u32], shared: &Mutex<Vec<u32>>) {
    for item in items {
        let mut guard = shared.lock().unwrap();
        guard.push(*item);
    }
}
";
        let findings = lock_findings(source);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].location.line_start, 5);
    }

    #[test]
    fn per_item_locks_are_expected_work() {
        let source = r"
use std::sync::Mutex;
fn touch(slots: &[Mutex<u32>]) {
    for slot in slots {
        *slot.lock().unwrap() += 1;
    }
}
";
        assert!(lock_findings(source).is_empty());
    }

    #[test]
    fn lock_outside_any_loop_is_not_flagged() {
        let source = r"
use std::sync::Mutex;
fn once(shared: &Mutex<u32>) {
    *shared.lock().unwrap() += 1;
}
";
        assert!(lock_findings(source).is_empty());
    }
}
