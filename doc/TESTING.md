<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Testing

What is checked, how, and — more importantly — how each check is shown
to be capable of failing.

## Contents

- [The rule](#the-rule)
- [Layers](#layers)
- [W3C conformance](#w3c-conformance)
- [Fuzzing](#fuzzing)
- [Property tests](#property-tests)
- [Two entry points, one scanner](#two-entry-points-one-scanner)
- [Miri](#miri)
- [Coverage](#coverage)
- [Documentation is tested too](#documentation-is-tested-too)
- [Running everything](#running-everything)

## The rule

**A check that cannot fail is not a check.** Every significant defect
found in this crate has been of one kind: something that appeared to be
verified and was not.

- A benchmark that had not compiled since it was written, because
  `cargo test` does not build benches.
- A conformance loader that silently dropped 159 tests and reported a
  number for the rest.
- Five fuzz targets with no seed corpus, doing 100,000 runs per second
  against an empty input and finding nothing.
- An encoding module at 5% coverage from its own tests, which measured
  78.7% because conformance documents happened to run through it.
- A README asserting behaviour that had changed, whose doctests were
  green because they were compiled from a *different copy* of the file.

So: when a test is added here, it is expected to be shown failing
first — by breaking the code it covers and watching it go red. Where
that has been done, the commit says so.

## Layers

| Layer | What it catches | Where |
|---|---|---|
| Unit tests | Module-level logic, error paths | `src/**/*.rs` |
| Integration tests | The public API as a caller sees it | `tests/` |
| Doctests | Examples in the README and rustdoc | compiled from source |
| Examples | Whole programs, run in CI | `examples/` |
| Property tests | Invariants over generated input | `tests/properties.rs` |
| Conformance | Agreement with the specification | `conformance/` |
| Fuzzing | Panics and hangs on hostile input | `fuzz/` |
| Miri | Undefined behaviour | CI |

## W3C conformance

The XML Conformance Test Suite (`xmlts20130923`, 2,585 tests) is
downloaded, verified against a pinned SHA-256, and run on every push.

```
overall  2533 pass, 24 fail, 0 panic, 28 unsupported, 0 blocked
         99.1% of 2557 decided (98.9% coverage of 2585)
```

Both numbers are reported together, always. A pass rate on a thin
denominator is how a runner flatters itself: skip everything hard and
100% is easy. The coverage figure is the denominator made visible.

**The suite is ratcheted.** `baselines/w3c-xml.tsv` records the
expected outcome of every test, and the run fails on a regression *and*
on an unreviewed improvement. A test that starts passing is good news
that still has to be looked at, because the usual cause is the runner
having stopped running it.

Outcomes are kept apart rather than collapsed:

- **Pass / Fail / Panic** count towards the pass rate.
- **Unsupported** — a feature not implemented — is excluded from the
  rate and reported in coverage.
- **Blocked** — the harness could not decide, e.g. a file the suite
  references but never shipped.

A panic is always the worst outcome. A caller cannot catch it, so an
input that causes one is a denial of service.

The scoring arithmetic is itself tested: reclassifying a test as
`Unsupported` must leave the pass rate untouched and show up as lost
coverage, and an empty run must report 0.0 rather than NaN — NaN
compares false against every threshold, so a ratchet built on it
accepts anything.

## Fuzzing

Five libFuzzer targets:

| Target | What it exercises |
|---|---|
| `parse` | The parser on arbitrary bytes |
| `tree_walk` | Every accessor on whatever tree came out |
| `parse_limits` | The parser with limits derived from the input |
| `xpath_compile` | The expression parser |
| `xpath_eval` | Evaluation against a fixed document |

Each has a **tracked seed corpus**. This matters more than it sounds:
unseeded, the targets did 100,000 runs in under a second and covered
almost nothing, because random bytes are not XML. Seeding took edge
coverage from zero to roughly 900–1,100 per target.

```bash
cargo +nightly fuzz run parse -- -max_total_time=300
```

## Property tests

Example-based tests check the cases someone thought of. Properties
state what must hold for *every* input and let proptest search for the
counterexample. Failures are recorded in `proptest-regressions/` and
replayed forever.

Among them:

- No input causes a panic.
- An error offset always lands on a character boundary of the document
  it came from.
- `line_column` never panics, even given a different document from the
  one that produced the error.
- Whatever XPath prints, parses back to the same number.

## Two entry points, one scanner

`parse` builds a tree; `stream::Reader` yields events. They run the
same scanner, and `tests/streaming.rs` exists to keep that true rather
than to assume it.

The tests do not check that the reader *works* — they check that it
**agrees**. Over a corpus of well-formed and malformed documents:

- the same documents are accepted and the same refused;
- a refusal carries the same `ErrorKind` at the same byte offset —
  not merely "both failed", which a reader that rejected everything
  would also satisfy;
- the same element sequence and the same text come out;
- `Limits` mean the same thing, checked across every combination of
  `max_depth` from 1 to 7 and nesting from 1 to 9.

Three defects were found by writing them, all of the same kind: state
the tree parser keeps for a document, the reader rebuilt for every
event. The internal subset was discarded between events, so every
DTD-declared entity was unknown to the reader and known to `parse`.
The entity-expansion budget was reset per event, so a bomb split
across fifty text nodes was handed the budget fifty times. And a
`CDATA` section consumed nothing and looped forever, because the
reader had its own copy of a decision the parser made elsewhere —
which is why the scanner is shared and not merely similar.

## Miri

Run over the test suite in CI. The crate forbids `unsafe`, so this is
belt and braces — but `libm` and the standard library are not, and
Miri has caught real differences in floating-point behaviour between
backends before. `log10`, `powf` and `round` were removed from the
XPath float shim because Rust does not specify their precision and
Miri, libm and the host disagreed by 2 ULP on `17.49`.

## Coverage

≥95% of lines, gated in CI, measured with `cargo-llvm-cov`. Currently
**97.4%**.

Two exclusions, both narrow and deliberate: `conformance/src/bin/`,
whose two `main()` functions shell out to the network and to `tar`.
Everything else in the harness is included, because it decides whether
a conformance test passed and a scoring bug would misreport the
library's own headline numbers.

The coverage job downloads the W3C suite first. Without it, `dtd.rs`
and `encoding.rs` are barely executed and the figure measures which
test data was available rather than what the library does.

## Documentation is tested too

- Every ```rust block in the README is compiled and run as a doctest.
- A CI check fails if the repo-root README and the crate README differ
  — only one of them is doctested, and when they drift it is always
  the other one that goes stale.
- A test parses the limits table out of the README and compares every
  cell against the profile it names, and fails if a public limit is
  missing from the table.
- A CI check runs every example under coverage instrumentation and
  fails if any public function is not called by at least one of them.

## Running everything

```bash
./scripts/gate.sh          # everything below, in fail-fastest order
```

That script exists because three separate pushes went out red on checks
that were green locally, and each time the cause was the same: the gate
being run by hand was a *subset* of CI. `no_std` and the feature
powerset were the ones that did it — a `Vec` that resolves through the
`std` prelude compiles until it does not, and nothing in a default
`cargo test` notices.

The individual commands:

```bash
cargo test --workspace --all-features        # 385 tests + 22 doctests
cargo test --no-default-features             # the no_std surface
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all --check
cargo llvm-cov --workspace --all-features --fail-under-lines 95 \
  --ignore-filename-regex 'conformance/src/bin/'
cargo run -p oxml-conformance --bin download # once
cargo test -p oxml-conformance --release
python3 scripts/check-example-coverage.py
./scripts/check-readmes-match.sh
cargo +nightly miri test -p oxml
cargo +nightly fuzz run parse -- -max_total_time=60
```
