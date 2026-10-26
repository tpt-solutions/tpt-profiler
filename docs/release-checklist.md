# Release checklist

Written for: maintainers preparing a release. This is a checklist only. Each
step is a decision for a maintainer, and none of the publishing steps run
from the repository automatically.

## Before tagging

1. Confirm the working tree is clean and on the release branch.
2. Run every gate, in the same order as CI:

   ```text
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo build -p tpt-profiler-model --no-default-features
   cargo deny check
   cargo doc --workspace --no-deps
   ```

   The MSRV job (`rust-version` in the root `Cargo.toml`) and the Windows,
   Linux and macOS matrix run in CI and must be green there too.
3. Run the ignored scale test once and record the timing:
   `cargo test -p tpt-profiler --lib three_hundred -- --ignored --nocapture`.
4. Review `CHANGELOG.md`. Rename `[Unreleased]` to the version and date, and
   add a fresh empty `[Unreleased]` above it.
5. Check the JSON schema version. If any persisted or emitted shape changed,
   bump it and update the golden fixtures in
   `crates/tpt-profiler-report/tests/fixtures/` (spec §69).
6. Check that the version in every `Cargo.toml` matches, including the path
   dependencies between the workspace crates.

## Versioning

* Patch: fixes only, no change to any output shape.
* Minor: new commands or detectors, and additive JSON fields.
* Major: any breaking change to a command, flag, exit code or schema.

Scoring weights and the priority formula are not an API contract (spec §27),
so changing them is a minor release with a note in the changelog.

## Publishing (manual, maintainer only)

These steps are outward-facing and need explicit approval for each release.

1. Create the annotated tag `vX.Y.Z` on the release commit.
2. Publish the crates in dependency order: `tpt-profiler-model` first, then
   the crates it depends on, ending with `tpt-profiler-cli`. Wait for each
   crate to appear in the index before publishing the next one.
3. Confirm `cargo install tpt-profiler-cli` works on Windows, Linux and macOS
   from the published crates, not from the checkout.
4. Attach prebuilt binaries from CI to the GitHub release, if that is part of
   the release.

## After publishing

1. Confirm the new version appears on crates.io and that docs.rs built.
2. Reopen `[Unreleased]` in the changelog for the next cycle.
3. Record anything that went wrong in this checklist, so the next release
   avoids it.
