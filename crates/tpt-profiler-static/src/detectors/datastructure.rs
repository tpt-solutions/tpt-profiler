//! Data-structure mismatch detection (spec §11): membership lookups on
//! list-like collections inside loops, and tiny fixed `HashMap`s.
//!
//! These are deliberately *recommendations, not prescriptions* (spec
//! §11): `contains` in a loop is only a mismatch when the collection is
//! a linear `Vec` and the loop is hot; without type information the
//! detector stays at `Low` confidence and relies on suppression comments
//! plus runtime evidence to raise certainty.

use syn::visit::Visit;
use tpt_profiler_model::{Category, Confidence, SourceLocation};

use crate::context::FileContext;
use crate::engine::{Analyzer, line_of};
use crate::finding::{Finding, render};

/// Flags workload/data-structure mismatches (spec §11).
#[derive(Debug)]
pub struct DataStructureDetector;

impl Analyzer for DataStructureDetector {
    fn name(&self) -> &'static str {
        "datastructure"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = DataStructureVisitor {
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
struct DataStructureVisitor {
    depth: u32,
    findings: Vec<Finding>,
}

impl<'ast> Visit<'ast> for DataStructureVisitor {
    crate::engine::impl_loop_depth!();

    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        if self.depth > 0 {
            let line = line_of(expr);
            if expr.method == "contains" {
                let receiver = render(&expr.receiver);
                if is_simple_identifier(&receiver) {
                    self.findings.push(Finding::new(
                        "datastructure",
                        Category::DataStructure,
                        format!(
                            "`{receiver}.contains(..)` inside a loop is O(n) per iteration \
                             on a list-like collection; if this is a `Vec`, a `HashSet`/\
                             `BTreeSet` turns the loop into O(1) lookups (spec §11)"
                        ),
                        SourceLocation::at_line(String::new(), line),
                        Confidence::Low,
                    ));
                }
            }
        }
        syn::visit::visit_expr_method_call(self, expr);
    }

    fn visit_expr_call(&mut self, expr: &'ast syn::ExprCall) {
        // `HashMap::from([(k, v), …])` with a handful of entries: a
        // fixed-size mapping is better served by an array or `match`.
        let rendered = render(&expr.func);
        if rendered.ends_with("HashMap::from") {
            if let Some(FirstArrayLen(len)) = expr.args.iter().find_map(first_array_len) {
                if len <= 4 {
                    let entries = if len == 1 { "entry" } else { "entries" };
                    self.findings.push(Finding::new(
                        "datastructure",
                        Category::DataStructure,
                        format!(
                            "`HashMap::from` with only {len} {entries} pays hashing overhead \
                             for a tiny fixed mapping; an array or `match` may be faster \
                             (spec §11)"
                        ),
                        SourceLocation::at_line(String::new(), line_of(expr)),
                        Confidence::Low,
                    ));
                }
            }
        }
        syn::visit::visit_expr_call(self, expr);
    }
}

fn is_simple_identifier(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(|ch| ch.is_alphanumeric() || ch == '_')
        && text.chars().next().is_some_and(|ch| !ch.is_ascii_digit())
}

/// Newtype so `find_map` can return the computed length.
struct FirstArrayLen(usize);

fn first_array_len(expr: &syn::Expr) -> Option<FirstArrayLen> {
    if let syn::Expr::Array(array) = expr {
        return Some(FirstArrayLen(array.elems.len()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StaticEngine;

    fn findings_of(source: &str) -> Vec<Finding> {
        let engine = StaticEngine::with_default_analyzers();
        let (findings, _) = engine.analyze_str("ds.rs", source);
        findings
            .into_iter()
            .filter(|finding| finding.detector == "datastructure")
            .collect()
    }

    #[test]
    fn contains_on_identifier_in_loop_is_flagged() {
        let source = r"
fn lookup(needles: &[u32], haystack: &[u32]) -> bool {
    for needle in needles {
        if haystack.contains(needle) {
            return true;
        }
    }
    false
}
";
        let findings = findings_of(source);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].confidence, Confidence::Low);
        assert_eq!(findings[0].category, Category::DataStructure);
    }

    #[test]
    fn contains_outside_loop_is_not_flagged() {
        let source = r"
fn once(haystack: &[u32]) -> bool {
    haystack.contains(&7)
}
";
        assert!(findings_of(source).is_empty());
    }

    #[test]
    fn tiny_hash_map_literal_is_flagged() {
        let source = r"
fn codes() -> std::collections::HashMap<u32, char> {
    std::collections::HashMap::from([(48, '0'), (49, '1')])
}
";
        let findings = findings_of(source);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].description.contains("2 entries"));
    }

    #[test]
    fn field_access_receivers_are_not_flagged() {
        let source = r"
fn lookup(needles: &[u32], table: &Tables) -> bool {
    for needle in needles {
        if table.allowed.contains(needle) {
            return true;
        }
    }
    false
}
struct Tables { allowed: Vec<u32> }
";
        // `table.allowed` is not a simple identifier; type unknown, so
        // the detector stays silent instead of guessing (spec §9).
        assert!(findings_of(source).is_empty());
    }
}
