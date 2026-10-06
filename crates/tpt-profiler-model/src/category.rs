//! Optimisation categories (spec §26).

use core::fmt;

/// An optimisation category (spec §26).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Category {
    /// CPU time and instruction efficiency.
    Cpu,
    /// Vectorisation opportunities (spec §12).
    Simd,
    /// Multi-core parallelism (spec §13).
    Parallelism,
    /// Memory usage and layout (spec §14).
    Memory,
    /// Heap allocation behaviour (spec §9.1).
    Allocation,
    /// Cache locality (spec §15).
    Cache,
    /// Algorithmic complexity (spec §2).
    Algorithm,
    /// Data-structure mismatch (spec §11).
    DataStructure,
    /// File/network I/O patterns (spec §16).
    Io,
    /// Serialisation cost (spec §17).
    Serialisation,
    /// Dependency graph cost (spec §18).
    Dependency,
    /// Binary size (spec §19).
    BinarySize,
    /// Compile time (spec §20).
    CompileTime,
    /// WebAssembly characteristics (spec §21).
    Wasm,
    /// GPU suitability (spec §22).
    Gpu,
    /// Architecture-level issues (spec §57).
    Architecture,
    /// Concurrency and synchronisation (spec §57).
    Concurrency,
}

impl Category {
    /// All categories, in spec §26 order.
    pub const ALL: [Category; 17] = [
        Self::Cpu,
        Self::Simd,
        Self::Parallelism,
        Self::Memory,
        Self::Allocation,
        Self::Cache,
        Self::Algorithm,
        Self::DataStructure,
        Self::Io,
        Self::Serialisation,
        Self::Dependency,
        Self::BinarySize,
        Self::CompileTime,
        Self::Wasm,
        Self::Gpu,
        Self::Architecture,
        Self::Concurrency,
    ];

    /// The uppercase slug used in deterministic IDs (spec §25), e.g.
    /// `SIMD`, `DATA_STRUCTURE`.
    #[must_use]
    pub const fn as_slug(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Simd => "SIMD",
            Self::Parallelism => "PARALLELISM",
            Self::Memory => "MEMORY",
            Self::Allocation => "ALLOCATION",
            Self::Cache => "CACHE",
            Self::Algorithm => "ALGORITHM",
            Self::DataStructure => "DATA_STRUCTURE",
            Self::Io => "IO",
            Self::Serialisation => "SERIALISATION",
            Self::Dependency => "DEPENDENCY",
            Self::BinarySize => "BINARY_SIZE",
            Self::CompileTime => "COMPILE_TIME",
            Self::Wasm => "WASM",
            Self::Gpu => "GPU",
            Self::Architecture => "ARCHITECTURE",
            Self::Concurrency => "CONCURRENCY",
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_slug())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString as _;

    #[test]
    fn slugs_are_unique_uppercase_without_spaces() {
        let mut slugs: alloc::vec::Vec<&str> = Category::ALL
            .iter()
            .map(|category| category.as_slug())
            .collect();
        let count = slugs.len();
        slugs.sort_unstable();
        slugs.dedup();
        assert_eq!(slugs.len(), count, "slugs must be unique");
        for slug in slugs {
            assert!(!slug.contains(' '), "slug {slug} must not contain spaces");
            assert!(
                slug.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'),
                "slug {slug} must be uppercase snake case"
            );
        }
    }

    #[test]
    fn display_matches_slug() {
        assert_eq!(Category::Simd.to_string(), "SIMD");
        assert_eq!(Category::DataStructure.to_string(), "DATA_STRUCTURE");
    }
}
