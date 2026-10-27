//! I/O detection (spec §16): a file read or opened on every iteration of a
//! loop with a loop-invariant path, forced flushes or syncs per iteration,
//! and unbuffered `write_all` calls inside loops.
//!
//! Reading a different file per item is expected work, so file calls are
//! only flagged when their arguments do not depend on the loop. Without
//! type information the write check cannot tell a buffered writer from a
//! raw file, so it stays at `Low` confidence and says so in its message.

use syn::visit::Visit;
use tpt_profiler_model::{Category, Confidence, SourceLocation};

use crate::context::FileContext;
use crate::engine::{Analyzer, line_of};
use crate::finding::{Finding, render};

use super::{LoopBindings, callee_is};

/// Flags I/O patterns inside loops (spec §16).
#[derive(Debug)]
pub struct IoDetector;

impl Analyzer for IoDetector {
    fn name(&self) -> &'static str {
        "io"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = IoVisitor {
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

/// Free functions and constructors that open or read a file.
const FILE_CALLS: &[&str] = &[
    "fs::read",
    "fs::read_to_string",
    "File::open",
    "File::create",
];

#[derive(Debug)]
struct IoVisitor {
    bindings: LoopBindings,
    findings: Vec<Finding>,
}

impl IoVisitor {
    fn push(&mut self, line: u32, confidence: Confidence, description: String) {
        self.findings.push(Finding::new(
            "io",
            Category::Io,
            description,
            SourceLocation::at_line(String::new(), line),
            confidence,
        ));
    }
}

impl<'ast> Visit<'ast> for IoVisitor {
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
            if invariant && FILE_CALLS.iter().any(|target| callee_is(&rendered, target)) {
                let line = line_of(expr);
                self.push(
                    line,
                    Confidence::Medium,
                    format!(
                        "`{rendered}` reads or opens the same file on every loop iteration; \
                         read or open it once outside the loop (spec §16)"
                    ),
                );
            }
        }
        syn::visit::visit_expr_call(self, expr);
    }

    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        if self.bindings.in_loop() {
            let line = line_of(expr);
            match expr.method.to_string().as_str() {
                "flush" | "sync_all" | "sync_data" => self.push(
                    line,
                    Confidence::Medium,
                    format!(
                        "`{}` forces a synchronous flush on every loop iteration, \
                         defeating buffering; flush once after the loop (spec §16)",
                        render(expr)
                    ),
                ),
                "write_all" => self.push(
                    line,
                    Confidence::Low,
                    format!(
                        "`{}` inside a loop may issue one small unbuffered write per \
                         iteration; wrap the target in `BufWriter` if it is a raw file \
                         or socket (spec §16)",
                        render(expr)
                    ),
                ),
                _ => {}
            }
        }
        syn::visit::visit_expr_method_call(self, expr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StaticEngine;

    fn io_findings(source: &str) -> Vec<Finding> {
        let engine = StaticEngine::with_default_analyzers();
        let (findings, _) = engine.analyze_str("io.rs", source);
        findings
            .into_iter()
            .filter(|finding| finding.detector == "io")
            .collect()
    }

    #[test]
    fn invariant_file_read_in_loop_is_flagged() {
        let source = r#"
fn poll(rounds: u32) {
    let config = "app.toml";
    for _ in 0..rounds {
        let text = std::fs::read_to_string(config);
        consume(text);
    }
}
fn consume<T>(_value: T) {}
"#;
        let findings = io_findings(source);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].location.line_start, 5);
        assert_eq!(findings[0].confidence, Confidence::Medium);
    }

    #[test]
    fn invariant_file_open_in_while_loop_is_flagged() {
        let source = r#"
fn spin(mut n: u32) {
    while n > 0 {
        let handle = File::open("log.txt");
        consume(handle);
        n -= 1;
    }
}
fn consume<T>(_value: T) {}
"#;
        assert_eq!(io_findings(source).len(), 1);
    }

    #[test]
    fn reading_a_different_file_per_item_is_not_flagged() {
        let source = r"
fn load(paths: &[String]) {
    for path in paths {
        let text = std::fs::read_to_string(path);
        consume(text);
    }
}
fn consume<T>(_value: T) {}
";
        assert_eq!(io_findings(source).len(), 0);
    }

    #[test]
    fn path_derived_inside_the_loop_is_not_flagged() {
        let source = r#"
fn scan(entries: &[Entry]) {
    for entry in entries {
        let estimates = entry.path().join("estimates.json");
        let text = std::fs::read_to_string(&estimates);
        consume(text);
    }
}
struct Entry;
impl Entry { fn path(&self) -> std::path::PathBuf { todo!() } }
fn consume<T>(_value: T) {}
"#;
        assert_eq!(io_findings(source).len(), 0);
    }

    #[test]
    fn flush_in_loop_is_flagged() {
        let source = r"
fn emit(writer: &mut Sink, rows: &[u32]) {
    for row in rows {
        writer.write_row(row);
        writer.flush();
    }
}
struct Sink;
impl Sink { fn write_row(&mut self, _row: &u32) {} fn flush(&mut self) {} }
";
        assert_eq!(io_findings(source).len(), 1);
    }

    #[test]
    fn write_all_in_loop_is_low_confidence() {
        let source = r"
fn dump(out: &mut Vec<u8>, rows: &[&[u8]]) {
    for row in rows {
        out.write_all(row);
    }
}
";
        let findings = io_findings(source);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].confidence, Confidence::Low);
    }

    #[test]
    fn file_read_outside_loop_is_not_flagged() {
        let source = r"
fn load(path: &str) -> String {
    let text = std::fs::read_to_string(path);
    text
}
";
        assert_eq!(io_findings(source).len(), 0);
    }

    #[test]
    fn similarly_named_user_function_is_not_flagged() {
        let source = r#"
fn scan(rounds: u32) {
    for _ in 0..rounds {
        myfs::read("config");
    }
}
mod myfs { pub fn read(_p: &str) {} }
"#;
        assert_eq!(io_findings(source).len(), 0);
    }
}
