//! Per-file analysis context: unsafe regions (spec §54), numerical-code
//! regions (spec §55), and suppression comments.
//!
//! The engine builds one [`FileContext`] per parsed file in a pre-pass
//! and uses it to enrich and filter findings, so detectors stay focused
//! on their own patterns.

use std::collections::HashSet;

use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use crate::engine::line_of;

/// Line ranges of interest in one source file (1-based, inclusive).
#[derive(Debug, Default, Clone)]
pub struct FileContext {
    /// Name the engine reports findings under (path relative to the
    /// analysed root).
    pub file: String,
    unsafe_ranges: Vec<(u32, u32)>,
    numerical_ranges: HashSet<(u32, u32)>,
    suppressed_lines: HashSet<u32>,
}

/// Comment marker suppressing a finding on the same or adjacent line.
pub const IGNORE_MARKER: &str = "tpt-profiler:ignore";
/// Comment marker suppressing findings on the following line.
pub const IGNORE_NEXT_MARKER: &str = "tpt-profiler:ignore-next";

/// Transcendental/float-only functions that mark code as numerical
/// (spec §55).
const FLOAT_METHODS: &[&str] = &[
    "sqrt", "sqrtf32", "sqrtf64", "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh",
    "cosh", "tanh", "exp", "exp2", "ln", "log", "log2", "log10", "powf", "powi", "cbrt", "hypot",
    "floor", "ceil", "round", "trunc", "fract", "recip", "mul_add",
];

impl FileContext {
    /// Builds the context for one parsed file.
    #[must_use]
    pub fn build(file_name: &str, file: &syn::File, source: &str) -> Self {
        let mut context = Self {
            file: file_name.to_string(),
            ..Self::default()
        };
        context.scan_suppressions(source);
        let mut unsafe_visitor = UnsafeVisitor::default();
        visit::visit_file(&mut unsafe_visitor, file);
        context.unsafe_ranges = unsafe_visitor.ranges;
        let mut numerical_visitor = NumericalVisitor::default();
        visit::visit_file(&mut numerical_visitor, file);
        context.numerical_ranges = numerical_visitor.ranges;
        context
    }

    fn scan_suppressions(&mut self, source: &str) {
        for (index, line) in source.lines().enumerate() {
            let line_number = u32::try_from(index + 1).unwrap_or(u32::MAX);
            let comment = line.split_once("//").map(|(_, comment)| comment);
            if let Some(comment) = comment {
                if comment.contains(IGNORE_MARKER) {
                    self.suppressed_lines.insert(line_number);
                } else if comment.contains(IGNORE_NEXT_MARKER) {
                    self.suppressed_lines.insert(line_number + 1);
                }
            }
        }
    }

    /// Whether `line` sits inside an `unsafe` block or `unsafe fn`.
    #[must_use]
    pub fn is_unsafe(&self, line: u32) -> bool {
        in_ranges(&self.unsafe_ranges, line)
    }

    /// Whether `line` sits in a function that does floating-point
    /// arithmetic (spec §55: faster != equivalent).
    #[must_use]
    pub fn is_numerical(&self, line: u32) -> bool {
        self.numerical_ranges
            .iter()
            .any(|&(start, end)| line >= start && line <= end)
    }

    /// Whether findings on `line` are suppressed by comment (checked on
    /// the line itself and its immediate neighbours, so trailing markers
    /// work either side).
    #[must_use]
    pub fn is_suppressed(&self, line: u32) -> bool {
        self.suppressed_lines.contains(&line)
            || self.suppressed_lines.contains(&(line - 1))
            || self.suppressed_lines.contains(&(line + 1))
    }
}

fn in_ranges(ranges: &[(u32, u32)], line: u32) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| line >= start && line <= end)
}

/// Records line ranges of `unsafe` blocks and `unsafe fn` bodies
/// (spec §54).
#[derive(Default, Debug)]
struct UnsafeVisitor {
    ranges: Vec<(u32, u32)>,
}

impl<'ast> Visit<'ast> for UnsafeVisitor {
    fn visit_expr_unsafe(&mut self, expr: &'ast syn::ExprUnsafe) {
        let start = line_of(expr);
        let end = u32::try_from(expr.span().end().line).unwrap_or(u32::MAX);
        if end >= start {
            self.ranges.push((start, end));
        }
        visit::visit_expr_unsafe(self, expr);
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let start = line_of(item);
        let end = u32::try_from(item.span().end().line).unwrap_or(u32::MAX);
        if matches!(item.sig.safety, syn::Safety::Unsafe(_)) && end >= start {
            self.ranges.push((start, end));
        }
        visit::visit_item_fn(self, item);
    }
}

/// Marks enclosing function ranges as numerical when they contain
/// floating-point literals, float casts, or float-only method calls
/// (spec §55).
#[derive(Default, Debug)]
struct NumericalVisitor {
    // Every function on the stack encloses whatever is being visited,
    // so a float op records all stack ranges directly.
    fn_stack: Vec<(u32, u32)>,
    ranges: HashSet<(u32, u32)>,
}

impl NumericalVisitor {
    fn note_float(&mut self, line: u32) {
        for &range in &self.fn_stack {
            if line >= range.0 && line <= range.1 {
                self.ranges.insert(range);
            }
        }
    }
}

impl<'ast> Visit<'ast> for NumericalVisitor {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let start = line_of(item);
        let end = u32::try_from(item.span().end().line).unwrap_or(u32::MAX);
        self.fn_stack.push((start, end));
        visit::visit_item_fn(self, item);
        self.fn_stack.pop();
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        let start = line_of(item);
        let end = u32::try_from(item.span().end().line).unwrap_or(u32::MAX);
        self.fn_stack.push((start, end));
        visit::visit_impl_item_fn(self, item);
        self.fn_stack.pop();
    }

    fn visit_lit_float(&mut self, lit: &'ast syn::LitFloat) {
        self.note_float(line_of(lit));
        visit::visit_lit_float(self, lit);
    }

    fn visit_expr_cast(&mut self, expr: &'ast syn::ExprCast) {
        let target = render_type(&expr.ty);
        if target == "f32" || target == "f64" {
            self.note_float(line_of(expr));
        }
        visit::visit_expr_cast(self, expr);
    }

    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        if FLOAT_METHODS.contains(&expr.method.to_string().as_str()) {
            self.note_float(line_of(expr));
        }
        visit::visit_expr_method_call(self, expr);
    }
}

fn render_type(ty: &syn::Type) -> String {
    use quote::ToTokens;
    ty.to_token_stream().to_string().replace(' ', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context_of(source: &str) -> FileContext {
        let file = syn::parse_file(source).expect("parse");
        FileContext::build("test.rs", &file, source)
    }

    #[test]
    fn unsafe_blocks_and_fns_are_tagged() {
        let source = r"
fn plain() {
    let x = 1;
}

unsafe fn dangerous() {
    let y = raw_deref();
}

fn partially() {
    let a = 1;
    unsafe {
        let b = raw_deref();
    }
    let c = 2;
}
";
        let context = context_of(source);
        assert!(!context.is_unsafe(3));
        assert!(context.is_unsafe(7), "unsafe fn body");
        assert!(context.is_unsafe(13), "unsafe block");
        assert!(!context.is_unsafe(15), "after the block");
    }

    #[test]
    fn float_code_marks_whole_function_numerical() {
        let source = r"
fn integer_math() -> i32 {
    2 + 2
}

fn float_math(a: f64, b: f64) -> f64 {
    (a * b).sqrt() + 0.5
}

fn casty(x: i32) -> f32 {
    (x as f32) * 1.0
}
";
        let context = context_of(source);
        assert!(!context.is_numerical(3));
        assert!(context.is_numerical(7), "transcendental + literal");
        assert!(context.is_numerical(11), "as f32 cast + literal");
    }

    #[test]
    fn suppression_markers_cover_adjacent_lines() {
        let source = r"
fn a() {
    let x = vec![1]; // tpt-profiler:ignore
    // tpt-profiler:ignore-next
    let y = vec![2];
    let z = vec![3];
}
";
        let context = context_of(source);
        assert!(context.is_suppressed(3));
        assert!(context.is_suppressed(5));
        assert!(!context.is_suppressed(6));
    }
}
