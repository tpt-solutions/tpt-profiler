//! Built-in detectors (spec §9–17). Each is a small, individually tested
//! `syn` visitor; limits of span-only (no type info) analysis are
//! documented per detector.

use std::collections::HashSet;

use quote::ToTokens;
use syn::visit::Visit;

pub mod allocation;
pub mod clone;
pub mod datastructure;
pub mod io;
pub mod loops;
pub mod serialisation;
pub mod vector;

/// Whether a rendered callee path names `target`, matching whole path
/// segments and ignoring turbofish arguments: `std::fs::read` and
/// `serde_json::from_str::<T>` match `fs::read` and `serde_json::from_str`,
/// but `myfs::read` does not match `fs::read`.
pub(crate) fn callee_is(rendered: &str, target: &str) -> bool {
    let base = rendered.split("::<").next().unwrap_or(rendered);
    base.strip_suffix(target)
        .is_some_and(|prefix| prefix.is_empty() || prefix.ends_with("::"))
}

/// Names bound inside each enclosing loop, so a detector can tell repeated
/// work on loop-invariant input (worth flagging) from work on the
/// per-iteration item (expected).
#[derive(Debug, Default)]
pub(crate) struct LoopBindings {
    scopes: Vec<HashSet<String>>,
}

impl LoopBindings {
    /// Whether the visitor is inside at least one loop.
    pub(crate) fn in_loop(&self) -> bool {
        !self.scopes.is_empty()
    }

    /// Enters a loop. Every identifier bound by the loop pattern or
    /// anywhere in the body counts as loop-varying; over-collecting only
    /// suppresses findings, which is the safe direction.
    pub(crate) fn enter(&mut self, pattern: Option<&syn::Pat>, body: &syn::Block) {
        struct Bound(HashSet<String>);
        impl<'ast> Visit<'ast> for Bound {
            fn visit_pat_ident(&mut self, pat: &'ast syn::PatIdent) {
                self.0.insert(pat.ident.to_string());
                syn::visit::visit_pat_ident(self, pat);
            }
        }
        let mut bound = Bound(HashSet::new());
        if let Some(pattern) = pattern {
            bound.visit_pat(pattern);
        }
        bound.visit_block(body);
        self.scopes.push(bound.0);
    }

    /// Leaves the innermost loop.
    pub(crate) fn leave(&mut self) {
        self.scopes.pop();
    }

    /// Whether any of `arguments` mentions an identifier bound inside an
    /// enclosing loop.
    pub(crate) fn depends_on_loop<T: ToTokens>(&self, arguments: &[&T]) -> bool {
        let text: String = arguments
            .iter()
            .map(|argument| argument.to_token_stream().to_string())
            .collect::<Vec<_>>()
            .join(" ");
        text.split(|character: char| !(character.is_alphanumeric() || character == '_'))
            .any(|word| self.scopes.iter().any(|scope| scope.contains(word)))
    }
}
