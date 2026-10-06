//! Stable hashing: FNV-1a (32-bit).
//!
//! `core::hash::DefaultHasher` is explicitly *not* stable across Rust
//! versions, and pulling a hash crate into the `no_std` model crate for
//! one function is not worth it — so the workspace standardises on a
//! local FNV-1a. It is deterministic across platforms, compiler versions,
//! and releases, which is exactly what deterministic opportunity IDs
//! (spec §25, §75) require. Do not use it where adversarial collision
//! resistance matters; it is a bucket hash, not a cryptographic one.

/// FNV-1a 32-bit offset basis.
pub const FNV_OFFSET_BASIS: u32 = 0x811C_9DC5;

/// FNV-1a 32-bit prime.
pub const FNV_PRIME: u32 = 0x0100_0193;

/// Computes the FNV-1a 32-bit hash of `bytes`.
///
/// Deterministic on every platform and Rust version (spec §75).
#[must_use]
#[allow(
    clippy::cast_lossless,
    reason = "u32::from is not const-callable yet; u8 as u32 is lossless"
)]
pub const fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash = FNV_OFFSET_BASIS;
    let mut index = 0;
    while index < bytes.len() {
        hash ^= bytes[index] as u32;
        hash = hash.wrapping_mul(FNV_PRIME);
        index += 1;
    }
    hash
}

/// Computes the FNV-1a 32-bit hash of a string's UTF-8 bytes.
#[must_use]
pub fn fnv1a_str(value: &str) -> u32 {
    fnv1a(value.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        // Standard FNV-1a 32-bit test vectors.
        assert_eq!(fnv1a(b""), 0x811C_9DC5);
        assert_eq!(fnv1a(b"a"), 0xE40C_292C);
        assert_eq!(fnv1a(b"foobar"), 0xBF9C_F968);
    }

    #[test]
    fn str_hash_matches_byte_hash() {
        assert_eq!(fnv1a_str("foobar"), fnv1a(b"foobar"));
    }

    #[test]
    fn hash_is_deterministic_and_differs_by_input() {
        assert_eq!(
            fnv1a_str("SIMD|mdct::transform|arithmetic loop"),
            fnv1a_str("SIMD|mdct::transform|arithmetic loop")
        );
        assert_ne!(fnv1a_str("SIMD|a|b"), fnv1a_str("SIMD|b|a"));
    }
}
