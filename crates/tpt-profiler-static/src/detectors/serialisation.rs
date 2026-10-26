//! Serialisation detection (spec §17): encoding or decoding the same
//! loop-invariant value repeatedly inside a loop, where the work could be
//! hoisted or done once.
//!
//! Encoding or decoding each item is expected work and is not flagged.
//! Only well-known serde format crates (JSON, TOML, YAML, binary and XML) are matched, so ordinary functions
//! that happen to be called `from_str` stay quiet.

use syn::visit::Visit;
use tpt_profiler_model::{Category, Confidence, SourceLocation};

use crate::context::FileContext;
use crate::engine::{Analyzer, line_of};
use crate::finding::{Finding, render};

use super::{LoopBindings, callee_is};

/// Flags serde encode/decode calls on loop-invariant input (spec §17).
#[derive(Debug)]
pub struct SerialisationDetector;

impl Analyzer for SerialisationDetector {
    fn name(&self) -> &'static str {
        "serialisation"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = SerialisationVisitor {
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

/// Format crates and the entry points that encode or decode with them.
const FORMAT_CALLS: &[&str] = &[
    "serde_json::to_string",
    "serde_json::to_string_pretty",
    "serde_json::to_vec",
    "serde_json::to_writer",
    "serde_json::from_str",
    "serde_json::from_slice",
    "serde_json::from_reader",
    "serde_json::from_value",
    "serde_json::to_value",
    "toml::to_string",
    "toml::from_str",
    "serde_yaml::to_string",
    "serde_yaml::from_str",
    "bincode::serialize",
    "bincode::deserialize",
    "rmp_serde::to_vec",
    "rmp_serde::from_slice",
    "ciborium::into_writer",
    "ciborium::from_reader",
    "quick_xml::de::from_str",
    "quick_xml::se::to_string",
    "serde_xml_rs::from_str",
    "serde_xml_rs::to_string",
];

#[derive(Debug)]
struct SerialisationVisitor {
    bindings: LoopBindings,
    findings: Vec<Finding>,
}

impl<'ast> Visit<'ast> for SerialisationVisitor {
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

    fn visit_expr_call(&mut self, expr: &'ast syn::ExprCall) {
        if self.bindings.in_loop() {
            let rendered = render(&expr.func);
            let arguments: Vec<&syn::Expr> = expr.args.iter().collect();
            let invariant = !self.bindings.depends_on_loop(&arguments);
            if invariant
                && FORMAT_CALLS
                    .iter()
                    .any(|target| callee_is(&rendered, target))
            {
                self.findings.push(Finding::new(
                    "serialisation",
                    Category::Serialisation,
                    format!(
                        "`{rendered}` encodes or decodes the same value on every loop \
                         iteration; do it once outside the loop (spec §17)"
                    ),
                    SourceLocation::at_line(String::new(), line_of(expr)),
                    Confidence::Medium,
                ));
            }
        }
        syn::visit::visit_expr_call(self, expr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StaticEngine;

    fn serialisation_findings(source: &str) -> Vec<Finding> {
        let engine = StaticEngine::with_default_analyzers();
        let (findings, _) = engine.analyze_str("ser.rs", source);
        findings
            .into_iter()
            .filter(|finding| finding.detector == "serialisation")
            .collect()
    }

    #[test]
    fn invariant_json_decode_in_loop_is_flagged() {
        let source = r#"
fn poll(rounds: u32) {
    let fixed = String::from("{}");
    for _ in 0..rounds {
        let value: Record = serde_json::from_str(&fixed).unwrap();
        consume(value);
    }
}
struct Record;
fn consume(_r: Record) {}
"#;
        let findings = serialisation_findings(source);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].location.line_start, 5);
    }

    #[test]
    fn invariant_xml_decode_in_loop_is_flagged() {
        let source = r#"
fn poll(rounds: u32) {
    let fixed = String::from("<a/>");
    for _ in 0..rounds {
        let value: Record = quick_xml::de::from_str(&fixed).unwrap();
        consume(value);
    }
}
struct Record;
fn consume(_r: Record) {}
"#;
        assert_eq!(serialisation_findings(source).len(), 1);
    }

    #[test]
    fn invariant_turbofish_call_in_loop_is_flagged() {
        let source = r#"
fn poll(rounds: u32) {
    let fixed = String::from("{}");
    for _ in 0..rounds {
        let value = serde_json::from_str::<Record>(&fixed);
        consume(value);
    }
}
struct Record;
fn consume<T>(_v: T) {}
"#;
        assert_eq!(serialisation_findings(source).len(), 1);
    }

    #[test]
    fn invariant_encode_in_while_loop_is_flagged() {
        let source = r"
fn encode(mut n: u32) {
    while n > 0 {
        let text = toml::to_string(&n);
        consume(text);
        n -= 1;
    }
}
fn consume<T>(_v: T) {}
";
        assert_eq!(serialisation_findings(source).len(), 1);
    }

    #[test]
    fn decoding_each_item_is_not_flagged() {
        let source = r"
fn parse_all(lines: &[String]) {
    for line in lines {
        let value: Record = serde_json::from_str(line).unwrap();
        consume(value);
    }
}
struct Record;
fn consume(_r: Record) {}
";
        assert!(serialisation_findings(source).is_empty());
    }

    #[test]
    fn serialisation_outside_loop_is_not_flagged() {
        let source = r"
fn once(text: &str) -> Record {
    serde_json::from_str(text).unwrap()
}
struct Record;
";
        assert!(serialisation_findings(source).is_empty());
    }

    #[test]
    fn user_function_named_like_a_format_call_is_not_flagged() {
        let source = r"
fn scan(rounds: u32) {
    for _ in 0..rounds {
        from_str(&String::new());
    }
}
fn from_str(_s: &String) {}
";
        assert!(serialisation_findings(source).is_empty());
    }
}
