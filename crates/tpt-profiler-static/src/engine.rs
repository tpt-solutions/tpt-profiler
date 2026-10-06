//! The static analysis engine (spec §9–13): analyzer registry, source
//! walking, parsing, and deterministic result assembly (spec §67, §75).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use syn::spanned::Spanned;
use syn::visit::Visit;
use tpt_profiler_model::error::{Error, ParseError};

use crate::context::FileContext;
use crate::detectors;
use crate::finding::{Finding, sort_findings};

/// One registered analyzer (spec §67): consumes a parsed file plus its
/// [`FileContext`] and produces standardised findings.
pub trait Analyzer {
    /// Registry name (stable, used in reports and suppressions).
    fn name(&self) -> &'static str;

    /// Analyses one parsed file.
    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding>;
}

/// Result of analysing a repository or source string.
#[derive(Debug, Default, Clone)]
pub struct StaticReport {
    /// All findings, in deterministic order (spec §75).
    pub findings: Vec<Finding>,
    /// Files that failed to parse; analysis continues regardless
    /// (spec §74, §9: never abort the run).
    pub parse_errors: Vec<ParseError>,
    /// Number of source files analysed.
    pub files_analyzed: usize,
}

impl StaticReport {
    /// Whether any findings were produced. A clean file is a *result*,
    /// not an error (spec §74).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Registry of analyzers with the default detector set (spec §62 static
/// scope: allocation, clone, data structure, loops, SIMD, parallelism).
#[derive(Default)]
pub struct StaticEngine {
    analyzers: Vec<Box<dyn Analyzer>>,
}

impl std::fmt::Debug for StaticEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaticEngine")
            .field("analyzer_count", &self.analyzers.len())
            .finish()
    }
}

impl StaticEngine {
    /// Engine with all built-in detectors registered.
    #[must_use]
    pub fn with_default_analyzers() -> Self {
        let mut engine = Self::default();
        engine.register(Box::new(detectors::allocation::AllocationDetector));
        engine.register(Box::new(detectors::clone::CloneDetector));
        engine.register(Box::new(detectors::datastructure::DataStructureDetector));
        engine.register(Box::new(detectors::loops::LoopHoistingDetector));
        engine.register(Box::new(detectors::vector::SimdDetector));
        engine.register(Box::new(detectors::vector::ParallelismDetector));
        engine
    }

    /// Adds an analyzer to the registry.
    pub fn register(&mut self, analyzer: Box<dyn Analyzer>) {
        self.analyzers.push(analyzer);
    }

    /// Analyses a single source string (used directly by tests and by
    /// callers that already have sources).
    ///
    /// Findings are filtered by suppression comments, enriched with
    /// unsafe/numerical context, and returned in deterministic order.
    #[must_use]
    pub fn analyze_str(&self, file_name: &str, source: &str) -> (Vec<Finding>, Vec<ParseError>) {
        let mut findings = Vec::new();
        let mut parse_errors = Vec::new();
        match syn::parse_file(source) {
            Ok(file) => {
                let context = FileContext::build(file_name, &file, source);
                for analyzer in &self.analyzers {
                    findings.extend(analyzer.analyze(&file, &context));
                }
                findings.retain(|finding| !context.is_suppressed(finding.location.line_start));
                for finding in &mut findings {
                    finding.location.unsafe_region = context.is_unsafe(finding.location.line_start);
                    finding.numerical_context = context.is_numerical(finding.location.line_start);
                }
            }
            Err(error) => {
                parse_errors.push(ParseError::InvalidSyntax {
                    path: file_name.to_string(),
                    line: u32::try_from(error.span().start().line).unwrap_or(u32::MAX),
                    message: error.to_string(),
                });
            }
        }
        sort_findings(&mut findings);
        (findings, parse_errors)
    }

    /// Walks `root` (gitignore-aware) and analyses every Rust source
    /// file. Parse failures are collected, never fatal (spec §9).
    pub fn analyze_path(&self, root: &Path) -> Result<StaticReport, Error> {
        let mut files: Vec<PathBuf> = walk_source_files(root).collect();
        files.sort();
        let mut report = StaticReport {
            files_analyzed: files.len(),
            ..StaticReport::default()
        };
        for path in &files {
            let source = std::fs::read_to_string(path).map_err(|error| {
                Error::Parse(ParseError::InvalidSyntax {
                    path: path.display().to_string(),
                    line: 0,
                    message: error.to_string(),
                })
            })?;
            let relative = path.strip_prefix(root).unwrap_or(path);
            let (mut findings, parse_errors) =
                self.analyze_str(&relative.to_string_lossy(), &source);
            report.findings.append(&mut findings);
            report.parse_errors.extend(parse_errors);
        }
        sort_findings(&mut report.findings);
        Ok(report)
    }
}

/// Gitignore-aware Rust source walk (spec §7, §72).
pub fn walk_source_files(root: &Path) -> impl Iterator<Item = PathBuf> {
    ignore::WalkBuilder::new(root)
        .hidden(true)
        .require_git(false)
        .build()
        .filter_map(|entry| {
            let path = entry.ok()?.into_path();
            (path.is_file() && path.extension().is_some_and(|ext| ext == "rs")).then_some(path)
        })
}

/// 1-based line of any syn node (spans are `usize`; model lines are
/// `u32`).
pub(crate) fn line_of<N: Spanned>(node: &N) -> u32 {
    u32::try_from(node.span().start().line).unwrap_or(u32::MAX)
}

/// Adds standard loop-depth tracking hooks to a `Visit` impl.
/// The visitor must have a `depth: u32` field; visitors with extra
/// per-loop logic write their own hooks instead.
macro_rules! impl_loop_depth {
    () => {
        fn visit_expr_for_loop(&mut self, expr: &'ast syn::ExprForLoop) {
            self.depth += 1;
            syn::visit::visit_expr_for_loop(self, expr);
            self.depth -= 1;
        }

        fn visit_expr_while(&mut self, expr: &'ast syn::ExprWhile) {
            self.depth += 1;
            syn::visit::visit_expr_while(self, expr);
            self.depth -= 1;
        }

        fn visit_expr_loop(&mut self, expr: &'ast syn::ExprLoop) {
            self.depth += 1;
            syn::visit::visit_expr_loop(self, expr);
            self.depth -= 1;
        }
    };
}

pub(crate) use impl_loop_depth;

/// Collects identifiers bound by a loop pattern (`for x in …`), so
/// detectors can recognise uses of the loop variable.
pub(crate) fn loop_variables(pattern: &syn::Pat) -> HashSet<String> {
    struct PatIdents(HashSet<String>);
    impl<'ast> Visit<'ast> for PatIdents {
        fn visit_pat_ident(&mut self, pat: &'ast syn::PatIdent) {
            self.0.insert(pat.ident.to_string());
            syn::visit::visit_pat_ident(self, pat);
        }
    }
    let mut visitor = PatIdents(HashSet::new());
    visitor.visit_pat(pattern);
    visitor.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_failures_are_reported_not_fatal() {
        let engine = StaticEngine::with_default_analyzers();
        let (findings, errors) = engine.analyze_str("broken.rs", "fn broken( {");
        assert!(findings.is_empty());
        assert_eq!(errors.len(), 1);
        let ParseError::InvalidSyntax { path, line, .. } = &errors[0] else {
            panic!("unexpected error variant: {:?}", errors[0]);
        };
        assert_eq!(path, "broken.rs");
        assert!(*line > 0);
    }

    #[test]
    fn analysis_is_deterministic_across_runs() {
        let source = r#"
fn hot(items: &[u32], names: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        let name = names[(*item) as usize].clone();
        out.push(name.to_string());
        if items.contains(&item) {
            out.push(format!("{item}"));
        }
    }
    out
}
"#;
        let engine = StaticEngine::with_default_analyzers();
        let (first, _) = engine.analyze_str("a.rs", source);
        let (second, _) = engine.analyze_str("a.rs", source);
        assert_eq!(first, second);
        assert!(!first.is_empty());
        // Deterministic order: sorted by line.
        let lines: Vec<u32> = first
            .iter()
            .map(|finding| finding.location.line_start)
            .collect();
        let mut sorted = lines.clone();
        sorted.sort_unstable();
        assert_eq!(lines, sorted);
    }
}
