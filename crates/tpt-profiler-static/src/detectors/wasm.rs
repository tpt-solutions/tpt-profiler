//! WebAssembly detection (spec §21, Phase 4.1), from source alone:
//!
//! * **native-only assumptions**: threads, files, processes and sockets
//!   used outside a `cfg` gate, which do not exist on `wasm32`;
//! * **JS boundary crossings**: `wasm-bindgen`, `js-sys` and `web-sys`
//!   calls made inside a loop, where each call crosses into JavaScript.
//! * **SIMD availability**: native intrinsics outside a target gate, and
//!   `wasm32` SIMD intrinsics in a file that never names `simd128`, so the
//!   feature is probably not enabled where those intrinsics are compiled.
//!
//! The target is not known to a source scan, so native-only findings are
//! Low confidence. Any item carrying a `cfg` that names a target is treated
//! as gated and skipped, which also skips its native code on native builds.

use std::collections::HashSet;

use syn::visit::Visit;
use tpt_profiler_model::{Category, Confidence, SourceLocation};

use crate::context::FileContext;
use crate::engine::{Analyzer, line_of};
use crate::finding::{Finding, render};

use super::{LoopBindings, callee_is};

/// Callees that need a native OS: threads, files, processes, sockets.
const NATIVE_ONLY: &[&str] = &[
    "thread::spawn",
    "fs::read",
    "fs::read_to_string",
    "fs::write",
    "File::open",
    "File::create",
    "Command::new",
    "TcpStream::connect",
    "TcpListener::bind",
];

/// Crates whose calls cross into JavaScript.
const JS_CRATES: &[&str] = &["wasm_bindgen::", "js_sys::", "web_sys::"];

/// Whether an item's attributes gate it on a target (`cfg(target_arch…)`,
/// `cfg(target_os…)`, `cfg(target_family…)`).
fn is_target_gated(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        if !attribute.path().is_ident("cfg") {
            return false;
        }
        let condition = quote::ToTokens::to_token_stream(&attribute.meta).to_string();
        ["target_arch", "target_os", "target_family"]
            .iter()
            .any(|key| condition.contains(key))
    })
}

/// Flags native-only std calls that are not behind a target gate.
#[derive(Debug)]
pub struct NativeOnlyDetector;

impl Analyzer for NativeOnlyDetector {
    fn name(&self) -> &'static str {
        "wasm_native_only"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = NativeVisitor {
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
struct NativeVisitor {
    findings: Vec<Finding>,
}

impl<'ast> Visit<'ast> for NativeVisitor {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !is_target_gated(&item.attrs) {
            syn::visit::visit_item_fn(self, item);
        }
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if !is_target_gated(&item.attrs) {
            syn::visit::visit_impl_item_fn(self, item);
        }
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !is_target_gated(&item.attrs) {
            syn::visit::visit_item_mod(self, item);
        }
    }

    fn visit_expr_call(&mut self, expr: &'ast syn::ExprCall) {
        let rendered = render(&expr.func);
        if let Some(target) = NATIVE_ONLY
            .iter()
            .find(|target| callee_is(&rendered, target))
        {
            self.findings.push(Finding::new(
                "wasm_native_only",
                Category::Wasm,
                format!(
                    "`{rendered}` needs a native OS (`{target}`) and will not work on wasm32 \
                     unless it is gated with `cfg(not(target_arch = \"wasm32\"))` (spec §21)"
                ),
                SourceLocation::at_line(String::new(), line_of(expr)),
                Confidence::Low,
            ));
        }
        syn::visit::visit_expr_call(self, expr);
    }
}

/// Flags x86 or AArch64 intrinsics outside a target gate. wasm32 has its
/// own `simd128` intrinsics, so such code needs a gate or a fallback
/// (spec §21: WASM SIMD availability).
#[derive(Debug)]
pub struct SimdIntrinsicDetector;

impl Analyzer for SimdIntrinsicDetector {
    fn name(&self) -> &'static str {
        "wasm_simd_intrinsics"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = IntrinsicVisitor {
            flagged_lines: HashSet::new(),
            findings: Vec::new(),
            wasm32_use: None,
        };
        visitor.visit_file(file);
        // Attributes and `cfg` arguments are tokens, so this sees
        // `target_feature(enable = "simd128")` and comments are excluded.
        let mentions_simd128 = quote::ToTokens::to_token_stream(file)
            .to_string()
            .contains("simd128");
        if !mentions_simd128 {
            if let Some((line, rendered)) = visitor.wasm32_use.take() {
                visitor.findings.push(Finding::new(
                    "wasm_simd_intrinsics",
                    Category::Wasm,
                    format!(
                        "`{rendered}` is a wasm32 SIMD intrinsic, but this file never names \
                         `simd128`; enable it with `#[target_feature(enable = \"simd128\")]` or \
                         build with `-C target-feature=+simd128` (spec §21)"
                    ),
                    SourceLocation::at_line(String::new(), line),
                    Confidence::Low,
                ));
            }
        }
        for finding in &mut visitor.findings {
            finding.location.file.clone_from(&context.file);
        }
        visitor.findings
    }
}

#[derive(Debug)]
struct IntrinsicVisitor {
    /// One finding per line, however many paths the line names.
    flagged_lines: HashSet<u32>,
    findings: Vec<Finding>,
    /// The first `arch::wasm32` path seen, with its line.
    wasm32_use: Option<(u32, String)>,
}

impl<'ast> Visit<'ast> for IntrinsicVisitor {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !is_target_gated(&item.attrs) {
            syn::visit::visit_item_fn(self, item);
        }
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if !is_target_gated(&item.attrs) {
            syn::visit::visit_impl_item_fn(self, item);
        }
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !is_target_gated(&item.attrs) {
            syn::visit::visit_item_mod(self, item);
        }
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let rendered = render(path);
        if rendered.contains("arch::wasm32") && self.wasm32_use.is_none() {
            self.wasm32_use = Some((line_of(path), rendered.clone()));
        }
        if rendered.contains("arch::x86") || rendered.contains("arch::aarch64") {
            let line = line_of(path);
            if self.flagged_lines.insert(line) {
                self.findings.push(Finding::new(
                    "wasm_simd_intrinsics",
                    Category::Wasm,
                    format!(
                        "`{rendered}` is a native SIMD intrinsic; wasm32 needs its own `simd128` code or a portable fallback behind `cfg` (spec §21)"
                    ),
                    SourceLocation::at_line(String::new(), line),
                    Confidence::Low,
                ));
            }
        }
        syn::visit::visit_path(self, path);
    }
}

/// Flags JS-boundary calls made inside a loop.
#[derive(Debug)]
pub struct JsBoundaryDetector;

impl Analyzer for JsBoundaryDetector {
    fn name(&self) -> &'static str {
        "wasm_js_boundary"
    }

    fn analyze(&self, file: &syn::File, context: &FileContext) -> Vec<Finding> {
        let mut visitor = JsVisitor {
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
struct JsVisitor {
    bindings: LoopBindings,
    findings: Vec<Finding>,
}

impl<'ast> Visit<'ast> for JsVisitor {
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
        let rendered = render(&expr.func);
        if self.bindings.in_loop() && JS_CRATES.iter().any(|krate| rendered.starts_with(krate)) {
            self.findings.push(Finding::new(
                "wasm_js_boundary",
                Category::Wasm,
                format!(
                    "`{rendered}` crosses the JS boundary on every loop iteration; batch the \
                     work on the Rust side and cross once (spec §21)"
                ),
                SourceLocation::at_line(String::new(), line_of(expr)),
                Confidence::Low,
            ));
        }
        syn::visit::visit_expr_call(self, expr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StaticEngine;

    fn wasm_findings(source: &str, detector: &str) -> Vec<Finding> {
        let engine = StaticEngine::with_default_analyzers();
        let (findings, _) = engine.analyze_str("wasm.rs", source);
        findings
            .into_iter()
            .filter(|finding| finding.detector == detector)
            .collect()
    }

    #[test]
    fn ungated_file_access_is_flagged_as_native_only() {
        let source = r#"
fn load() -> String {
    std::fs::read_to_string("data.txt").unwrap()
}
"#;
        let findings = wasm_findings(source, "wasm_native_only");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].category, Category::Wasm);
    }

    #[test]
    fn target_gated_native_code_is_not_flagged() {
        let source = r#"
#[cfg(not(target_arch = "wasm32"))]
fn load() -> String {
    std::fs::read_to_string("data.txt").unwrap()
}
"#;
        assert!(wasm_findings(source, "wasm_native_only").is_empty());
    }

    #[test]
    fn js_call_inside_a_loop_is_flagged() {
        let source = r"
fn draw(points: &[f64]) {
    for point in points {
        js_sys::Math::sqrt(*point);
    }
}
";
        let findings = wasm_findings(source, "wasm_js_boundary");
        assert_eq!(findings.len(), 1, "{findings:?}");
    }

    #[test]
    fn native_intrinsics_are_flagged_once_per_line() {
        let source = r"
fn add(a: u32, b: u32) -> u32 {
    unsafe { core::arch::x86_64::_mm_add_ps(a, b) }
}
";
        let findings = wasm_findings(source, "wasm_simd_intrinsics");
        assert_eq!(findings.len(), 1, "{findings:?}");
    }

    #[test]
    fn wasm32_intrinsics_without_simd128_are_flagged() {
        let source = r"
fn add(a: v128, b: v128) -> v128 {
    core::arch::wasm32::u32x4_add(a, b)
}
";
        let findings = wasm_findings(source, "wasm_simd_intrinsics");
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].description.contains("simd128"));
    }

    #[test]
    fn wasm32_intrinsics_with_simd128_enabled_are_not_flagged() {
        let source = r#"
#[target_feature(enable = "simd128")]
fn add(a: v128, b: v128) -> v128 {
    core::arch::wasm32::u32x4_add(a, b)
}
"#;
        assert!(wasm_findings(source, "wasm_simd_intrinsics").is_empty());
    }

    #[test]
    fn js_call_outside_a_loop_is_not_flagged() {
        let source = r"
fn draw(point: f64) -> f64 {
    js_sys::Math::sqrt(point)
}
";
        assert!(wasm_findings(source, "wasm_js_boundary").is_empty());
    }
}
