# Security Policy

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅        |
| < 0.1   | ❌ (pre-release) |

## Reporting a vulnerability

**Do not open a public issue for security vulnerabilities.**

Contact the maintainers privately (security contact: TPT Solutions,
`security@tpt.solutions` — or the maintainer's listed contact on the
repository) with:

* affected version / commit;
* a description of the issue;
* reproduction steps or a proof of concept.

You will receive an acknowledgement within 7 days. We aim to release a fix
for critical issues within 30 days.

## Security model

`tpt-profiler` is a local analysis tool. Relevant boundaries:

* **No network.** The tool performs no network access and sends no
  telemetry; source code never leaves the machine (spec §70). Any future
  network-capable feature will be explicit and opt-in. This is enforced by
  a dependency ban list (`deny.toml`) and CI.
* **Executes build/bench targets on request.** Commands such as
  `runtime`, `bench`, and `build` compile and execute the *target
  project's* code (including its `build.rs` and benchmarks), exactly as
  `cargo build` / `cargo bench` would. Only run them on projects you
  trust. Pure static analysis (`static`) never executes target code.
* **No unsafe code.** The profiler is compiled with
  `unsafe_code = "forbid"`.
* **No source modification.** The tool never writes to the analysed
  repository's source (spec §53).
* **Local artefacts.** Results are written under `.tpt-profiler/` in the
  analysed repository; treat that directory as untrusted input when
  loading existing results (it may come from a cloned repo).
