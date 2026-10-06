# Local check helper — mirrors CI (see .github/workflows/ci.yml).
# `just check` is the pre-push minimum; `just all` mirrors full CI.
default: check

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

deny:
    cargo deny check licenses bans sources

deny-advisories:
    cargo deny check advisories

doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace

no-std:
    cargo build -p tpt-profiler-model --no-default-features

check: fmt-check clippy test

all: check deny doc no-std
