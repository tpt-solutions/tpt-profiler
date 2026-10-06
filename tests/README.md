# Integration tests & fixtures

Cross-crate integration tests and fixture workspaces live here
(spec §61, §76). This directory is excluded from the workspace
(`exclude` in the root `Cargo.toml`) so the fixture packages are never
built as members.

Layout (grows with the MVP):

```text
tests/
├── README.md            (this file)
└── fixtures/            fixture projects used by discovery/static tests
    └── single-crate/    minimal one-crate Cargo project
```

Rules for fixtures:

* Each fixture is a self-contained, minimal Cargo project.
* Fixtures must be tiny — they exist to exercise discovery and detectors
  deterministically, not to build slowly.
* Fixture `Cargo.toml`s use edition 2021 (or older) so they do not impose
  the workspace MSRV on their own toolchains.
* Never add fixtures' `Cargo.lock`/`target/` to git.
