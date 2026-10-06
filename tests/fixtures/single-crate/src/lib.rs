//! Minimal fixture crate for discovery and static-analysis tests.

/// Adds one to the input. Deliberately trivial.
#[must_use]
pub const fn add_one(x: u64) -> u64 {
    x + 1
}

#[cfg(test)]
mod tests {
    #[test]
    fn adds_one() {
        assert_eq!(super::add_one(1), 2);
    }
}
