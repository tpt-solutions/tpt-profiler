//! Allocation detection (spec §9.1): heap allocation inside loops,
//! repeated `Vec` growth, missing capacity reservation, temporary
//! collections, and repeated formatting.
//!
//! Without type information these are pattern heuristics — hence
//! `Medium` confidence at most, with suppression comments as the escape
//! hatch for false positives.

use syn::visit::Visit;
use tpt_profiler_model::{Category, Confidence, SourceLocation};

use crate::context::FileContext;
use crate::engine::{Analyzer, line_of};
use crate::finding::{Finding, render};

/// Flags allocation patterns inside loops (spec §9.1).
#[derive(Debug)]
pub struct AllocationDetector;

impl Analyzer for AllocationDetector {
    fn name(&self) -> &'static str {
        "allocation"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = AllocationVisitor {
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
struct AllocationVisitor {
    depth: u32,
    findings: Vec<Finding>,
}

impl AllocationVisitor {
    fn push(&mut self, line: u32, description: String) {
        self.findings.push(Finding::new(
            "allocation",
            Category::Allocation,
            description,
            SourceLocation::at_line(String::new(), line),
            Confidence::Medium,
        ));
    }
}

impl<'ast> Visit<'ast> for AllocationVisitor {
    crate::engine::impl_loop_depth!();

    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        if self.depth > 0 {
            let line = line_of(expr);
            match expr.method.to_string().as_str() {
                "to_string" | "to_owned" => self.push(
                    line,
                    format!(
                        "`{}` allocates a new `String` on every loop iteration; \
                         format or reuse a buffer outside the loop (spec §9.1)",
                        render(expr)
                    ),
                ),
                "collect" => self.push(
                    line,
                    "`.collect()` inside a loop allocates a fresh collection per iteration; \
                     consider collecting once outside the loop (spec §9.1)"
                        .to_string(),
                ),
                _ => {}
            }
        }
        syn::visit::visit_expr_method_call(self, expr);
    }

    fn visit_expr_call(&mut self, expr: &'ast syn::ExprCall) {
        if self.depth > 0 {
            let rendered = render(&expr.func);
            let line = line_of(expr);
            if rendered.ends_with("Vec::new") || rendered.ends_with("String::new") {
                self.push(
                    line,
                    format!(
                        "`{rendered}()` inside a loop re-allocates each iteration and grows \
                         by doubling; reuse the buffer or use `with_capacity` (spec §9.1)"
                    ),
                );
            }
        }
        syn::visit::visit_expr_call(self, expr);
    }

    fn visit_macro(&mut self, mac: &syn::Macro) {
        if self.depth > 0 {
            let name = mac
                .path
                .segments
                .last()
                .map_or(String::new(), |segment| segment.ident.to_string());
            let line = line_of(mac);
            match name.as_str() {
                "format" => self.push(
                    line,
                    "format! inside a loop allocates a new `String` every iteration; \
                     reuse a `String` buffer with `write!` if this is hot (spec §9.1)"
                        .to_string(),
                ),
                "vec" => self.push(
                    line,
                    "vec! inside a loop allocates a fresh Vec every iteration; \
                     hoist it or reuse a buffer (spec §9.1)"
                        .to_string(),
                ),
                _ => {}
            }
        }
        syn::visit::visit_macro(self, mac);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StaticEngine;

    fn findings_of(source: &str) -> Vec<Finding> {
        let engine = StaticEngine::with_default_analyzers();
        let (findings, _) = engine.analyze_str("alloc.rs", source);
        findings
            .into_iter()
            .filter(|finding| finding.detector == "allocation")
            .collect()
    }

    #[test]
    fn vec_new_in_loop_is_flagged() {
        let source = r"
fn chunk(items: &[u32]) {
    for item in items {
        let scratch = Vec::new();
        process(scratch, item);
    }
}
fn process(_a: Vec<u32>, _b: &u32) {}
";
        let findings = findings_of(source);
        assert!(
            findings
                .iter()
                .any(|finding| finding.location.line_start == 4)
        );
    }

    #[test]
    fn format_and_to_string_in_loop_are_flagged() {
        // Note: a `format!` nested inside `println!(..)` tokens is
        // opaque to `syn` (macros are opaque token streams), so the
        // macro stays at statement position (spec §9.1).
        let source = r#"
fn log_all(ids: &[u32]) {
    for id in ids {
        let line = format!("id={id}");
        println!("{line}");
        let label = id.to_string();
        let _ = label;
    }
}
"#;
        let findings = findings_of(source);
        assert_eq!(findings.len(), 2, "format! + to_string: {findings:?}");
    }

    #[test]
    fn collect_in_loop_is_flagged() {
        let source = r"
fn join(rows: &[Row]) {
    for row in rows {
        let cells: Vec<String> = row.fields.iter().map(String::clone).collect();
        render(cells);
    }
}
fn render(_cells: Vec<String>) {}
struct Row { fields: Vec<String> }
struct String;
";
        let findings = findings_of(source);
        assert!(
            findings
                .iter()
                .any(|finding| finding.location.line_start == 4)
        );
    }

    #[test]
    fn allocation_outside_loop_is_not_flagged() {
        let source = r"
fn once() -> Vec<u32> {
    let scratch = Vec::new();
    scratch
}
";
        assert_eq!(findings_of(source).len(), 0);
    }

    #[test]
    fn vec_new_outside_loop_with_push_is_not_flagged_here() {
        // `Vec::new` outside a loop is normal growth; the detector only
        // targets per-iteration allocation (spec §9.1).
        let source = r"
fn build() -> Vec<u32> {
    let mut out = Vec::new();
    for i in 0..10 {
        out.push(i);
    }
    out
}
";
        assert_eq!(findings_of(source).len(), 0);
    }
}
