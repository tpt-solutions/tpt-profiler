//! Function sites for mapping runtime hotspots back to source (spec §8.5,
//! Phase 2.2). A profiler symbol such as `crate::module::sum` or
//! `<T as Trait>::method` is matched on its last path segment; only a name
//! that names exactly one function in the project is mapped, so an
//! ambiguous symbol is left unmapped rather than guessed.

use std::path::{Path, PathBuf};

use syn::visit::Visit;

use crate::engine::walk_source_files;

/// A function defined in the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSite {
    /// The function's own name, without its path.
    pub name: String,
    /// Source file, as found under the scanned root.
    pub file: PathBuf,
    /// 1-based line of the function name.
    pub line: u32,
}

/// Every named function (free functions and methods) under `root`. Files
/// that do not parse are skipped, as in the static analysis.
#[must_use]
pub fn function_sites(root: &Path) -> Vec<FunctionSite> {
    let mut sites = Vec::new();
    for file in walk_source_files(root) {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Ok(syntax) = syn::parse_file(&text) else {
            continue;
        };
        let mut visitor = SiteVisitor {
            file: &file,
            sites: &mut sites,
        };
        visitor.visit_file(&syntax);
    }
    sites.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    sites
}

/// The site a profiler symbol maps to, when exactly one function has that
/// name. Closures and other unnamed frames map to nothing.
#[must_use]
pub fn locate<'a>(symbol: &str, sites: &'a [FunctionSite]) -> Option<&'a FunctionSite> {
    let name = last_segment(symbol)?;
    let mut matches = sites.iter().filter(|site| site.name == name);
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

/// The final identifier of a demangled symbol: `a::b::c` and
/// `<T as Tr>::c` both give `c`. `None` for closures and empty frames.
fn last_segment(symbol: &str) -> Option<&str> {
    let tail = symbol.rsplit("::").next()?.trim();
    let tail = tail.split('<').next()?.trim();
    let valid = !tail.is_empty()
        && tail.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !tail.starts_with(|c: char| c.is_ascii_digit());
    valid.then_some(tail)
}

struct SiteVisitor<'a> {
    file: &'a Path,
    sites: &'a mut Vec<FunctionSite>,
}

impl SiteVisitor<'_> {
    fn record(&mut self, ident: &syn::Ident) {
        let line = u32::try_from(ident.span().start().line).unwrap_or(0);
        self.sites.push(FunctionSite {
            name: ident.to_string(),
            file: self.file.to_path_buf(),
            line,
        });
    }
}

impl<'ast> Visit<'ast> for SiteVisitor<'_> {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.record(&item.sig.ident);
        syn::visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.record(&item.sig.ident);
        syn::visit::visit_impl_item_fn(self, item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(name: &str) -> FunctionSite {
        FunctionSite {
            name: name.to_owned(),
            file: PathBuf::from("src/lib.rs"),
            line: 3,
        }
    }

    #[test]
    fn symbols_map_on_their_last_segment() {
        let sites = [site("sum"), site("fold")];
        assert_eq!(
            locate("hot_loop::sum", &sites).map(|s| s.name.as_str()),
            Some("sum")
        );
        assert_eq!(
            locate("<T as Tr>::fold", &sites).map(|s| s.name.as_str()),
            Some("fold")
        );
        assert!(locate("[unknown]", &sites).is_none());
        assert!(
            locate(
                "core::ops::function::FnOnce::call_once{{vtable.shim}}",
                &sites
            )
            .is_none()
        );
    }

    #[test]
    fn ambiguous_names_are_not_guessed() {
        let sites = [site("new"), site("new")];
        assert!(locate("Buffer::new", &sites).is_none());
    }

    #[test]
    fn functions_and_methods_are_found_in_a_project() {
        let root = std::env::temp_dir().join(format!("tpt-fn-sites-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).expect("src");
        std::fs::write(
            root.join("src").join("lib.rs"),
            "pub fn sum(v: &[u32]) -> u32 { v.iter().sum() }\nstruct S;\nimpl S {\n    fn scale(&self) {}\n}\n",
        )
        .expect("write");
        let found = function_sites(&root);
        let names: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"sum") && names.contains(&"scale"),
            "{found:?}"
        );
        let scale = found.iter().find(|s| s.name == "scale").expect("scale");
        assert_eq!(scale.line, 4);
        let _ = std::fs::remove_dir_all(&root);
    }
}
