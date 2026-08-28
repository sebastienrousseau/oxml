<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Roadmap

Where the 10/10 plan stands. Measured on 2026-08-23, against
`feat/v0.0.4`.

Every "done" here has a number or a check behind it. Anything without
one is listed as not done, however close it feels.

## Contents

- [Scorecard](#scorecard)
- [Done](#done)
- [Next](#next)
- [After that](#after-that)
- [Suite audit, 2026-08-28](#suite-audit-2026-08-28)
- [Candidates for 0.0.7](#candidates-for-007)
- [Not planned](#not-planned)
- [How to tell when something is finished](#how-to-tell-when-something-is-finished)

## Scorecard

| Axis | Target | Now | |
|---|---|---|---|
| Memory safety | No `unsafe` | `#![forbid(unsafe_code)]`, CI-checked | ✅ |
| `no_std` | Full, with `alloc` | Builds for 3 bare-metal targets in CI | ✅ |
| Panics on hostile input | Zero | 0 across 2,585 conformance documents, 6 fuzz targets, Miri | ✅ |
| Resource bounds | Configurable | 10 bounds, 3 profiles, per-document entity budget | ✅ |
| XXE | Structurally impossible | No file or socket code exists | ✅ |
| Line coverage | ≥95% | **97.2%**, gated | ✅ |
| Conformance | Published with denominator | **100% of 2,557 decided; 98.9% of 2,585 reach a decision** | ✅ |
| Allocations per node | ≤2 | **0.50** | ✅ |
| Throughput | <100 ms at load | **Ratios measured, absolute still not** — `benches/comparison.rs` reports 0.089× `quick-xml` (events) and 0.319× `roxmltree` (tree), stable to 3–5% under 10 CPU hogs and gated at 15%. MB/s still needs a quiet machine; `scripts/record-throughput.sh` refuses above 0.20 load per core and has never yet been able to record | 🟡 |
| XPath 1.0 | Complete | **All 13 axes and all 27 functions**, namespaces resolved | ✅ |
| Documentation | House style, all 6 crates | READMEs, `doc/`, examples, FAQs across all six | ✅ |
| Streaming | An entry point | **`stream::Reader`**, same scanner as the tree parser; holds 89% less at peak. Not yet from a reader | 🟡 |

## Done

**Entity replacement text is markup.** An entity referenced from
content is *included* per XML 1.0 §4.4.2 — its replacement text parsed
as content, not substituted as characters. Thirteen conformance
failures went at once, taking the pass rate from 98.6% to **99.1%**.
The replacement text is checked by the same scanner the document uses,
into a throwaway tree, so attribute-value rules, reserved
processing-instruction targets, name validity and comment termination
are enforced without being restated.

**Owned input.** `Document` owns its text and character data is
`(start, len)` ranges into it, so **0.50 allocations per node** —
down from 4.13 when counting began, via 1.13. Fewer than one per node
means most nodes cost none. No lifetime parameter on `Document`: see
[adr/0007](adr/0007-owned-strings-for-now.md) for why that mattered
more than it looks, and [design/owned-input.md](design/owned-input.md)
for what the design note got wrong.

**Correctness.** Ten defects fixed, each with a test that fails without
the fix: XML 1.0 and 1.1 line-ending normalisation, attribute-value
normalisation, XPath namespace resolution in name tests,
`local-name()`/`namespace-uri()` on attributes, a panic in
`Error::line_column`, unbounded parser recursion, quadratic entity
blowup, a comment before `<!DOCTYPE`, and three XPath 1.0 conformance
defects.

**Verification.** W3C suite with a ratcheted baseline; five seeded fuzz
targets; Miri; property tests; feature powerset; `no_std` on three
targets; 97.2% coverage with the conformance harness inside the floor.

**Performance.** 4.13 → **1.13 allocations per node**, held to a
ceiling by a test.

**Documentation.** All six crates to house style, with the claims made
checkable: README doctests, a check that the two README copies match, a
test that parses the limits table out of the README and compares it to
the code, a check that every public function is called by an example,
and every code block under `doc/` compiled as a test.

## Next

In the order that buys the most.

### 1. Throughput — measurable now, still unmeasured

The plan states its target in throughput, and until 0.0.6 **nothing
measured throughput at all**: the benchmarks reported time per
document, which cannot be checked against a figure in MB/s without
each document's size.

`benches/throughput.rs` now tells criterion the byte count, so it
reports a rate. Three shapes: markup-dominated, text-dominated, and
attribute-dominated.

There is still no published figure, and that is deliberate. The same
binary measured 14.7 and 123.1 MB/s on this machine on one day; the
difference was load. `scripts/record-throughput.sh` checks the
fifteen-minute load average against a fifth of a core before it
measures, and exits without a number when the machine is busy — it has
refused on every attempt so far, at between 0.7 and 1.5 per core.

A figure obtained by overriding that check would not be a
measurement.

This needs a quiet machine, not more code. Until then the performance
claim rests entirely on the allocation count, which is a proxy.

`doc/BENCHMARKS.md` states the method and the conditions a figure must
carry.

### 2. Conformance — done

**2,557 of 2,557 decided tests pass, with zero panics.** The count was
37 at the start of this cycle.

What remains is not a failure list but a coverage one: 28 tests reach
no decision — 14 want namespace processing switched off, 8 want
Namespaces 1.1, 6 want encodings beyond UTF-8, UTF-16 and ISO-8859-1.
Supporting any of them is a feature, not a fix, and the coverage
figure is published beside the rate so the distinction stays visible.

See [CONFORMANCE.md](CONFORMANCE.md) for what each change was.

### 3. Memoise entity validation

An entity's replacement text is checked once per *reference*, so a
document referencing one entity a thousand times parses it a thousand
times. Keying a cache on the entity name alone is unsound: validity
depends on the namespace bindings in scope, so `<!ENTITY e "<p:x/>">`
is well-formed where `p` is bound and not where it is not. The key has
to include the scope, and a wrong key accepts documents that should be
refused.

Unquantified: the machine available measured the same entity benchmark
between 1.45 and 3.76 ms in one state, so no honest before-and-after
could be taken. Measure it on a quiet machine before optimising it.

### 4. Streaming from a reader

`stream::Reader` now yields events over the same scanner with no tree
built, which is most of this item: measured against parsing the same
16,004-node document, it holds 191,967 bytes at peak against
1,809,822 — 89% less, and nearly all of what remains is the
normalised copy of the input. It was 92% before the document began
owning its input; the gap narrowed because the tree got cheaper.

That copy is what is left to remove. The reader takes a `&str`, so
"documents larger than memory" is still in *When not to use*, and
`quick-xml` is still the answer for a socket or a gigabyte file. The
work is an input abstraction that refills a buffer, plus deciding what
happens to a token that straddles a refill.

## After that

- **Serialisation and mutation.** One feature, not two. Round-tripping
  comments, entity references, attribute order and whitespace is most
  of the difficulty.
- **The 28 unsupported conformance tests**: 14 want namespace
  processing switched off, 8 want Namespaces 1.1, 6 want encodings
  beyond UTF-8/UTF-16/ISO-8859-1.
- **`xmlschema`: closing the remaining conformance failures.** The
  suite itself is no longer the gap — it runs the W3C XSD tests,
  39,420 of them, at 95.6% of decided. What remains is substitution
  groups and the undecidable corners of derivation validity.
- **`oxml-lsp`: the LSP transport.** The crate is named for a protocol
  it does not yet speak; `analyse()` and a linter are what exist.
- **`oxml-mcp`: a handle-based flow**, so a large document need not
  cross the boundary on every call.

## Suite audit, 2026-08-28

Measured across all six repositories rather than estimated. Every
figure below came from running something.

| Repo | Src | Tests | Coverage | Benches | Examples | Fuzz | CI jobs |
|---|---|---|---|---|---|---|---|
| `oxml` | 10,637 | 389 | 97.2% | 7 | 10 | 6 | 15 |
| `xmlschema` | 6,790 | 238 | ≥95% | 3 | 1 | **0** | 6 |
| `oxml-mcp` | 1,439 | — | 99.2% | **0** | **0** | 0 | 5 |
| `oxml-cli` | 729 | — | 99.2% | **0** | **0** | 0 | 5 |
| `oxml-wasm` | 517 | — | 100% core | **0** | **0** | 0 | 6 |
| `oxml-lsp` | 256 | — | 97.8% | **0** | 1 | 0 | 5 |

`oxml-wasm` reads 80.2% to `llvm-cov` because `src/lib.rs` holds the
`wasm-bindgen` exports, which cannot be instrumented natively. They
are covered by eleven `wasm_bindgen_test` cases run under
`wasm-pack test --node`, and `core.rs` is at 100%. The exclusion is
sound; the native figure is the misleading one.

### The gap that matters most

**`xmlschema` publishes 95.6% of 39,420 conformance tests and nothing
protects it.** Its `conformance/` crate has a runner and a downloader,
an **empty** `baselines/` directory, **no** `tests/` directory, and no
CI job that runs the suite. The headline figure is produced by
invoking a binary by hand.

`oxml` has exactly this in working order — a baseline that ratchets,
a test that fails on regression *and* on unreviewed improvement, and a
check that the published figures match what the harness prints. That
machinery already exists and needs porting, not inventing.

This is the same shape as the defects found in `oxml` this cycle: a
number that looks measured and is not checked.

### Documentation that contradicts the code

Both verified by running the thing, not by reading it.

- **`oxml-cli`**'s README says namespace prefixes on the command line
  are "Not yet". `--ns` exists, is in `--help`, works
  (`oxml query --ns m=urn:x '//m:a'` returns the node), and the error
  message for an unbound prefix *tells you to use it*.
- **`xmlschema`**'s ecosystem table lists `oxml-cli`, `oxml-lsp`,
  `oxml-mcp` and `oxml-wasm` as **Planned**. All four are published.

### Verified, so not a gap

The breaking change in 0.0.7 -- `Document::kind` returning a borrowed
view -- **breaks none of the five satellites**. All build and pass
against `main`: 209 tests, zero failures. The view was shaped so that
`Some(NodeKind::Text(t)) => t.trim()` and `a.value` keep compiling,
and they do. Earlier notes in this file warned they would break; that
warning was wrong and is withdrawn.

## Candidates for 0.0.7

In the order that buys the most, with the reason each is worth doing.

1. **Port the conformance ratchet to `xmlschema`.** A baseline file, a
   test that fails on drift in either direction, a figures check, and
   a CI job. Its largest claim is currently unguarded, and the code to
   guard it is written and working next door.

2. **Fuzz `xmlschema`.** It parses untrusted XSD -- the same threat
   model as `oxml`, which has six targets and found real defects with
   them. `xmlschema` has none. A schema is an input a caller did not
   write.

3. **Correct the two stale claims** above. Small, and exactly the kind
   of drift this suite keeps finding in itself.

4. **Type-aware attribute-value normalisation** in `oxml`. The last
   conformance failure: a tokenized `ATTLIST` type is stripped and
   collapsed where `CDATA` is not, so `xmlns:b=" urn:xyzzy "` declared
   `NMTOKEN` binds two prefixes to one namespace.

5. **Examples for `oxml-cli`, `oxml-mcp` and `oxml-wasm`.** `oxml`
   gates on every public function being executed by an example; the
   satellites have none at all. The gate is what keeps that true, so
   port it with them.

6. **Benchmarks for the four satellites.** None has any. `oxml-mcp` at
   1,439 lines does per-call document work that nothing measures.

7. **Streaming from a reader** in `oxml`. The remaining ❌: `Reader`
   takes a `&str`, so a document larger than memory is still out of
   reach.

8. **The absolute throughput figure.** Not code -- it needs a quiet
   machine, which is now a demonstrated conclusion rather than an
   excuse. See [BENCHMARKS.md](BENCHMARKS.md).

Deliberately *not* on this list: an LSP transport for `oxml-lsp`. It
is the crate's whole reason to exist and too large for a patch
release, and its README already says plainly that it is not yet a
language server.

## Not planned

- **XSLT.** Use `libxml`.
- **XPath 2.0 / 3.1.** This is 1.0.
- **Error recovery** for well-formedness. A parser that guesses at a
  malformed document produces a tree no two implementations agree on.
- **Fetching anything.** No network, no filesystem, ever. It is the
  property that makes XXE structurally impossible rather than
  configurably absent.

## How to tell when something is finished

The rule this project has converged on, the hard way:

> **A check that cannot fail is not a check.**

Every significant defect found here was something that appeared
verified and was not — a benchmark that had not compiled since it was
written, a conformance loader silently dropping 159 tests, fuzz targets
with no corpus, an encoding module at 5% coverage that measured 78.7%,
a README asserting behaviour that had been false for months because its
doctests were compiled from a different copy of the file.

So an item moves to Done when its check has been shown to fail: break
the code, watch it go red, put it back. Where that has been done, the
commit says so.
