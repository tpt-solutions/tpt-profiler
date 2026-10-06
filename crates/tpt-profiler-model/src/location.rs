//! Source locations with unsafe-region tagging (spec §54).

use core::fmt;

use alloc::string::String;

/// A location in the analysed source tree (spec §54).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SourceLocation {
    /// Path relative to the containing crate/project root.
    pub file: String,
    /// 1-based first line; `0` when unknown.
    pub line_start: u32,
    /// 1-based last line (inclusive); `0` when unknown.
    pub line_end: u32,
    /// Whether the region involves `unsafe` code (spec §54).
    pub unsafe_region: bool,
}

impl SourceLocation {
    /// A single-line, safe location (`line` for both bounds).
    #[must_use]
    pub fn at_line(file: impl Into<String>, line: u32) -> Self {
        Self {
            file: file.into(),
            line_start: line,
            line_end: line,
            unsafe_region: false,
        }
    }

    /// Marks this location as involving `unsafe` code.
    #[must_use]
    pub const fn with_unsafe_region(mut self) -> Self {
        self.unsafe_region = true;
        self
    }

    /// Human-readable `file:line` form (e.g. `src/transform.rs:142`).
    #[must_use]
    pub fn short(&self) -> String {
        if self.line_start == 0 {
            self.file.clone()
        } else {
            alloc::format!("{}:{}", self.file, self.line_start)
        }
    }
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line_start, self.line_end) {
            (0, _) | (_, 0) => f.write_str(&self.file),
            (start, end) if start == end => write!(f, "{}:{start}", self.file),
            (start, end) => write!(f, "{}:{start}-{end}", self.file),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::{String, ToString as _};

    #[test]
    fn display_forms() {
        let single = SourceLocation::at_line("src/a.rs", 3);
        assert_eq!(single.to_string(), "src/a.rs:3");
        let range = SourceLocation {
            file: String::from("src/b.rs"),
            line_start: 142,
            line_end: 188,
            unsafe_region: true,
        };
        assert_eq!(range.to_string(), "src/b.rs:142-188");
        assert!(range.short().contains("142"));
        let unknown = SourceLocation {
            file: String::from("src/c.rs"),
            line_start: 0,
            line_end: 0,
            unsafe_region: false,
        };
        assert_eq!(unknown.to_string(), "src/c.rs");
    }

    #[test]
    fn unsafe_flag_is_preserved() {
        let loc = SourceLocation::at_line("src/x.rs", 7).with_unsafe_region();
        assert!(loc.unsafe_region);
    }
}
