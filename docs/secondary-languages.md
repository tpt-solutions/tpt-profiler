# Secondary languages: feasibility

Written for: maintainers deciding whether C/C++ or Python analysis belongs in
the project (spec §3.2, Phase 4.8). Status: assessment only, nothing is
implemented.

## Recommendation

Start with a time-boxed spike on one language, not both. **Python** is the
better first target: the profiler's main value is ranking recommendations
from measurements, and Python workloads are where a static loop finding is
most likely to be wrong, so the spike tests the evidence model as much as
the parser. C/C++ comes second, because its build and macro expansion make
static findings weaker without the compiler's help.

## What the Rust analysis relies on

The static engine parses with `syn`, which is Rust-only. Every detector is
a `syn::visit::Visit` over the syntax tree. A second language needs a
parser that exposes the same shapes: loops, calls, method calls, ranges, and
index expressions. The detectors use these shapes, not Rust semantics.

Two things do not carry over unchanged:

* `Finding`, `Category`, `Evidence` and the opportunity engine are already
  language-neutral. They can be reused as they are.
* Heuristics that depend on Rust types, such as `Arc` versus struct clones
  (`clone` detector) and `Vec` in `collect::<Vec<_>>()`, have no direct
  equivalent. They would need per-language rules.

## Options for parsing

| Option | Covers | Licence | Notes |
|---|---|---|---|
| `tree-sitter` with `tree-sitter-python` | Python | MIT | Error-tolerant; one generic API for all languages; needs a C runtime build |
| `tree-sitter` with `tree-sitter-c` / `tree-sitter-cpp` | C, C++ | MIT | Same API; C++ grammar is large and slower to build |
| `rustpython-parser` | Python | MIT | Pure Rust; Python-only; less maintained recently |
| `libclang` bindings | C, C++ | Apache-2.0 with LLVM exception | Full semantics, but needs a matching LLVM install; outside the MIT-or-Apache policy's comfort zone |

The dependency policy in [CONTRIBUTING.md](../CONTRIBUTING.md) applies to each option:
`deny.toml` allows MIT and Apache-2.0 dual licences, and `libclang` would
need an explicit review before adoption.

**Preferred:** `tree-sitter` with the Python grammar. It is MIT, it gives a
single parser API for later C and C++ work, and it does not need a language
toolchain on the user's machine.

## What the Python spike would prove

1. Loop-invariant detection (`loops` and `io`) works on Python `for`/`while`
   and on calls such as `open()` or `json.loads()` inside a loop.
2. The privacy rule holds: the parser runs locally, with no network.
3. Findings from a second language still pass through the same confidence
   ceilings and the same `StaticPattern` evidence tier (spec §29).
4. A Python workload can be measured with the existing runtime path, so the
   measure-first rule (spec §81) applies unchanged.

The spike is done when a Python fixture produces the same kind of finding
as the Rust fixture, and the regression corpus gains a Python file with
expectations.

## What stays out of scope

* No cross-language call graph. A finding stays in the file it was found in.
* No type inference for Python. Duck typing means every call is unknown, so
  every finding is at most Medium confidence.
* No C/C++ until the Python spike has shipped and been reviewed.

## Open questions for maintainers

* Should a Python finding share a category with the Rust one for the same
  pattern (for example, allocation in loops), or get its own?
* Is `tree-sitter`'s C build step acceptable for the MSRV job and the
  `no_std` check? The `no_std` check covers only the model crate, so a
  parser in the static crate does not affect it, but the MSRV job does.
