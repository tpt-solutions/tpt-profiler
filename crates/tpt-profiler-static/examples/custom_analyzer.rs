//! A custom analyzer registered alongside the built-in detectors (spec §67).
//!
//! The analyzer flags `to_lowercase()` and `to_uppercase()` inside loops,
//! where each iteration allocates a new `String`. It is deliberately small:
//! copy it as a starting point, then see `docs/contributing-analyzers.md`.
//!
//! Run with `cargo run -p tpt-profiler-static --example custom_analyzer`.

use syn::visit::Visit;
use tpt_profiler_model::{Category, Confidence, SourceLocation};
use tpt_profiler_static::{Analyzer, FileContext, Finding, StaticEngine};

/// Flags case conversions made inside a loop.
#[derive(Debug)]
struct CaseConversionInLoop;

impl Analyzer for CaseConversionInLoop {
    fn name(&self) -> &'static str {
        "case_conversion_in_loop"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = Visitor {
            loop_depth: 0,
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
struct Visitor {
    loop_depth: usize,
    findings: Vec<Finding>,
}

impl<'ast> Visit<'ast> for Visitor {
    fn visit_expr_for_loop(&mut self, expr: &'ast syn::ExprForLoop) {
        self.loop_depth += 1;
        syn::visit::visit_expr_for_loop(self, expr);
        self.loop_depth -= 1;
    }

    fn visit_expr_while(&mut self, expr: &'ast syn::ExprWhile) {
        self.loop_depth += 1;
        syn::visit::visit_expr_while(self, expr);
        self.loop_depth -= 1;
    }

    fn visit_expr_loop(&mut self, expr: &'ast syn::ExprLoop) {
        self.loop_depth += 1;
        syn::visit::visit_expr_loop(self, expr);
        self.loop_depth -= 1;
    }

    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        let method = expr.method.to_string();
        if self.loop_depth > 0 && matches!(method.as_str(), "to_lowercase" | "to_uppercase") {
            let line = u32::try_from(syn::spanned::Spanned::span(expr).start().line).unwrap_or(0);
            self.findings.push(Finding::new(
                "case_conversion_in_loop",
                Category::Allocation,
                format!(
                    "`.{method}()` allocates a new String on every loop iteration; convert \
                     once outside the loop if the input does not change"
                ),
                SourceLocation::at_line(String::new(), line),
                Confidence::Low,
            ));
        }
        syn::visit::visit_expr_method_call(self, expr);
    }
}

const SAMPLE: &str = r"
fn normalise(names: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for name in names {
        out.push(name.to_lowercase());
    }
    out
}
";

fn main() {
    let mut engine = StaticEngine::with_default_analyzers();
    engine.register(Box::new(CaseConversionInLoop));
    let (findings, parse_errors) = engine.analyze_str("sample.rs", SAMPLE);
    for error in &parse_errors {
        eprintln!("parse error: {error}");
    }
    for finding in &findings {
        println!(
            "{}:{} [{}] {}",
            finding.location.file,
            finding.location.line_start,
            finding.detector,
            finding.description
        );
    }
}
