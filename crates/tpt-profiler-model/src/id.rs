//! Deterministic, reproducible opportunity IDs (spec §25).
//!
//! An ID has the form `CATEGORY-NNNNN`, e.g. `SIMD-04217`. The number is
//! derived from a stable FNV-1a hash (see [`crate::hash`]) over the
//! normalised `category|component|description` triple, so the same
//! finding produces the same ID on every machine and in every run —
//! IDs are content addresses, not counters (spec §75).
//!
//! Callers pass a category *slug* (uppercase, no separators; e.g.
//! `SIMD`, `ALLOCATION`, `DATA_STRUCTURE`). The category enum (spec §26)
//! will provide these slugs.

use crate::hash::fnv1a_str;
use alloc::format;
use alloc::string::String;
use core::fmt;

/// Number of decimal digits in the numeric part of an ID.
const NUMBER_WIDTH: usize = 5;

/// Mask limiting the hash to the numeric range representable in
/// [`NUMBER_WIDTH`] digits (`00000..=65535`).
const NUMBER_MASK: u32 = 0xFFFF;

/// A stable opportunity identifier such as `SIMD-04217`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OpportunityId(String);

impl OpportunityId {
    /// Generates the ID for a finding.
    ///
    /// * `category_slug` — uppercase category slug, used verbatim in the
    ///   ID (e.g. `SIMD`).
    /// * `component` — hierarchical component path (e.g. `mdct::transform`).
    /// * `description` — short, stable description of the opportunity.
    ///
    /// All three are normalised (lower-cased, whitespace collapsed) for
    /// hashing only; the emitted ID keeps the slug as given.
    ///
    /// # Panics
    /// Panics in debug builds if `category_slug` is empty or contains
    /// `-` or whitespace (it must be a slug); release builds normalise
    /// instead of panicking.
    #[must_use]
    pub fn generate(category_slug: &str, component: &str, description: &str) -> Self {
        debug_assert!(
            !category_slug.is_empty()
                && !category_slug.contains('-')
                && !category_slug.contains(char::is_whitespace),
            "category slug must be a slug like `SIMD`, got {category_slug:?}",
        );
        let key = format!(
            "{}|{}|{}",
            normalise(category_slug),
            normalise(component),
            normalise(description)
        );
        let number = fnv1a_str(&key) & NUMBER_MASK;
        Self(format!("{category_slug}-{number:0NUMBER_WIDTH$}"))
    }

    /// The ID as a string slice (`SIMD-04217`).
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OpportunityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Lower-cases ASCII, trims, and collapses whitespace runs to single
/// spaces, so wording/formatting churn does not change IDs.
fn normalise(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending_space = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(ch.to_ascii_lowercase());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_is_category_plus_five_digits() {
        let id =
            OpportunityId::generate("SIMD", "mdct::transform", "independent scalar arithmetic");
        let text = id.as_str();
        assert!(text.starts_with("SIMD-"), "unexpected: {text}");
        assert_eq!(text.len(), "SIMD-".len() + NUMBER_WIDTH);
        let digits = &text["SIMD-".len()..];
        assert!(
            digits.bytes().all(|b| b.is_ascii_digit()),
            "unexpected: {text}"
        );
    }

    #[test]
    fn ids_are_deterministic_across_normalisation() {
        let a = OpportunityId::generate("SIMD", "mdct::transform", "independent scalar arithmetic");
        let b = OpportunityId::generate(
            "SIMD",
            "mdct::transform",
            "Independent  Scalar\nArithmetic ", // different case/spacing only
        );
        assert_eq!(a, b);
    }

    #[test]
    fn different_content_changes_the_id() {
        let a = OpportunityId::generate("ALLOCATION", "parse_frame", "vec growth in loop");
        let b = OpportunityId::generate("ALLOCATION", "parse_frame", "string formatting in loop");
        assert_ne!(a, b);
    }

    #[test]
    fn sorting_is_deterministic() {
        let a = OpportunityId::generate("SIMD", "a", "one");
        let b = OpportunityId::generate("SIMD", "b", "two");
        let mut once = [b.clone(), a.clone()];
        once.sort();
        let mut twice = [b, a];
        twice.sort();
        // Hash-derived numbers are arbitrary but stable, so sorted order
        // must be reproducible across runs (spec §75).
        assert_eq!(once, twice);
        assert!(once.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn display_matches_inner_string() {
        let id = OpportunityId::generate("CPU", "x", "y");
        assert_eq!(format!("{id}"), id.as_str());
    }
}
