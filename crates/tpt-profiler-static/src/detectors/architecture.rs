//! Architecture detection (spec §57, Phase 4.4), from one file at a time:
//!
//! * **unnecessary conversion layers**: an owned value built only to be
//!   borrowed again (`x.to_string().as_str()`), or a `Vec` collected only to
//!   be iterated or measured (`….collect::<Vec<_>>().iter()`);
//! * **duplicate implementations**: two functions in one file whose bodies
//!   are token-for-token identical.
//!
//! Duplicates across files need the whole crate at once, which an
//! [`Analyzer`] does not see; that is left to a later cross-file pass. These
//! findings are structural hints, so they are Low or Medium confidence and
//! never measurements (spec §57).

use std::collections::HashMap;

use syn::visit::Visit;
use tpt_profiler_model::{Category, Confidence, SourceLocation};

use crate::context::FileContext;
use crate::engine::{Analyzer, line_of};
use crate::finding::{Finding, render};

/// Bodies with fewer statements than this are too small to call duplicates.
const MIN_DUPLICATE_STATEMENTS: usize = 3;

/// Flags conversions that build an owned value only to borrow it again.
#[derive(Debug)]
pub struct ConversionLayerDetector;

impl Analyzer for ConversionLayerDetector {
    fn name(&self) -> &'static str {
        "conversion_layer"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = ConversionVisitor {
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
struct ConversionVisitor {
    findings: Vec<Finding>,
}

impl ConversionVisitor {
    fn flag(&mut self, line: u32, description: String) {
        self.findings.push(Finding::new(
            "conversion_layer",
            Category::Architecture,
            description,
            SourceLocation::at_line(String::new(), line),
            Confidence::Medium,
        ));
    }
}

impl<'ast> Visit<'ast> for ConversionVisitor {
    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        if let syn::Expr::MethodCall(inner) = &*expr.receiver {
            let outer = expr.method.to_string();
            let inner_name = inner.method.to_string();
            if matches!(outer.as_str(), "as_str" | "as_bytes")
                && matches!(inner_name.as_str(), "to_string" | "to_owned")
            {
                self.flag(
                    line_of(expr),
                    format!(
                        "`{}` allocates an owned value only to borrow it with `{outer}`; \
                         borrow the original instead (spec §57)",
                        render(&expr.receiver)
                    ),
                );
            }
            if matches!(outer.as_str(), "iter" | "into_iter" | "len" | "is_empty")
                && inner_name == "collect"
                && inner
                    .turbofish
                    .as_ref()
                    .is_some_and(|turbofish| render(turbofish).contains("Vec"))
            {
                self.flag(
                    line_of(expr),
                    format!(
                        "`{}` collects into a Vec only to call `{outer}` on it; use the \
                         iterator directly (spec §57)",
                        render(&expr.receiver)
                    ),
                );
            }
        }
        syn::visit::visit_expr_method_call(self, expr);
    }
}

/// Flags functions whose body repeats an earlier function's body exactly.
#[derive(Debug)]
pub struct DuplicateBodyDetector;

impl Analyzer for DuplicateBodyDetector {
    fn name(&self) -> &'static str {
        "duplicate_body"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = DuplicateVisitor {
            seen: HashMap::new(),
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
struct DuplicateVisitor {
    /// Body text → name and line of the first function with that body.
    seen: HashMap<String, (String, u32)>,
    findings: Vec<Finding>,
}

impl DuplicateVisitor {
    fn record(&mut self, name: &str, block: &syn::Block, line: u32) {
        if block.stmts.len() < MIN_DUPLICATE_STATEMENTS {
            return;
        }
        let body = render(block);
        match self.seen.get(&body) {
            Some((first, first_line)) => self.findings.push(Finding::new(
                "duplicate_body",
                Category::Architecture,
                format!(
                    "`{name}` has the same body as `{first}` (line {first_line}); share one \
                     implementation instead of two copies (spec §57)"
                ),
                SourceLocation::at_line(String::new(), line),
                Confidence::Low,
            )),
            None => {
                self.seen.insert(body, (name.to_owned(), line));
            }
        }
    }
}

impl<'ast> Visit<'ast> for DuplicateVisitor {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.record(
            &item.sig.ident.to_string(),
            &item.block,
            line_of(&item.sig.ident),
        );
        syn::visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.record(
            &item.sig.ident.to_string(),
            &item.block,
            line_of(&item.sig.ident),
        );
        syn::visit::visit_impl_item_fn(self, item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StaticEngine;

    fn findings_for(source: &str, detector: &str) -> Vec<Finding> {
        let engine = StaticEngine::with_default_analyzers();
        let (findings, _) = engine.analyze_str("arch.rs", source);
        findings
            .into_iter()
            .filter(|finding| finding.detector == detector)
            .collect()
    }

    #[test]
    fn owned_value_built_only_to_be_borrowed_is_flagged() {
        let source = r"
fn label(name: &str) -> bool {
    name.to_string().as_str().is_empty()
}
";
        assert_eq!(findings_for(source, "conversion_layer").len(), 1);
    }

    #[test]
    fn vec_collected_only_to_be_iterated_is_flagged() {
        let source = r"
fn total(values: &[u32]) -> u32 {
    values.iter().map(|v| v + 1).collect::<Vec<_>>().iter().sum()
}
";
        assert_eq!(findings_for(source, "conversion_layer").len(), 1);
    }

    #[test]
    fn borrowing_the_original_is_not_flagged() {
        let source = r"
fn label(name: &str) -> bool {
    name.is_empty()
}
";
        assert_eq!(findings_for(source, "conversion_layer").len(), 0);
    }

    #[test]
    fn identical_bodies_in_one_file_are_flagged_once_per_copy() {
        let source = r"
fn first(values: &[u32]) -> u32 {
    let mut total = 0;
    for value in values {
        total += value;
    }
    total
}
fn second(values: &[u32]) -> u32 {
    let mut total = 0;
    for value in values {
        total += value;
    }
    total
}
";
        let findings = findings_for(source, "duplicate_body");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].description.contains("`second`"));
        assert!(findings[0].description.contains("`first`"));
    }

    #[test]
    fn small_bodies_are_not_called_duplicates() {
        let source = r"
fn a(x: u32) -> u32 { x + 1 }
fn b(x: u32) -> u32 { x + 1 }
";
        assert_eq!(findings_for(source, "duplicate_body").len(), 0);
    }
}
