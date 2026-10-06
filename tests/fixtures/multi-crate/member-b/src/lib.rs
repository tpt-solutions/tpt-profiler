//! Fixture member B, depends on member A via a workspace-internal path.

/// Uses member A.
pub fn b() -> u32 {
    tpt_fixture_member_a::a()
}
