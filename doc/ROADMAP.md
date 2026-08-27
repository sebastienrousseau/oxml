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
| Line coverage | ≥95% | **97.4%**, gated | ✅ |
| Conformance | Published with denominator | **99.6% of 2,557 decided; 98.9% of 2,585 reach a decision** | ✅ |
| Allocations per node | ≤2 | **0.50** | ✅ |
| Throughput | <100 ms at load | **Ratios measured, absolute still not** — `benches/comparison.rs` reports 0.089× `quick-xml` (events) and 0.319× `roxmltree` (tree), stable to 3–5% under 10 CPU hogs and gated at 15%. MB/s still needs a quiet machine; `scripts/record-throughput.sh` refuses above 0.20 load per core and has never yet been able to record | 🟡 |
| XPath 1.0 | Complete | **All 13 axes and all 27 functions**, namespaces resolved | ✅ |
| Documentation | House style, all 6 crates | READMEs, `doc/`, examples, FAQs across all six | ✅ |
| Streaming | An entry point | **`stream::Reader`**, same scanner as the tree parser; holds 92% less at peak. Not yet from a reader | 🟡 |

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
targets; 97.4% coverage with the conformance harness inside the floor.

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

### 2. Conformance — 11 failures, all about external content

Every remaining failure is the parser being **too permissive**; there
is no document in the suite it wrongly rejects.

The count was 37. Two more went when conditional sections in an
external subset stopped being swallowed: a section saying `CDATA`
where only `INCLUDE` or `IGNORE` is legal was accepted in silence,
because a declaration containing a parameter entity took a path that
reports anything unparseable as merely *incomplete*.

The thirteen before them shared one cause — an
entity's replacement text substituted as characters where XML 1.0
§4.4.2 requires it to be *included*, that is parsed as content — and
were fixed as one change. `crates/oxml/tests/entity_markup.rs` keeps
them fixed and records the distinction that makes the rule subtle: a
character reference in an entity's declaration is *included* there and
then, while a general entity reference is *bypassed*, so
`<!ENTITY e "&#38;">` is a trap and `<!ENTITY e "&amp;">` is not.

What is left all turns on a file oxml never reads:

| What the failure needs | Count |
|---|---|
| Standalone declarations against external content | 3 |
| Remaining §4.3.4 version-agreement cases | 3 |
| External identifier and entity-reference rules | 3 |
| An external DTD (`eduni/rmt-*`, `oasis/o-p09fail1`) | 2 |

No group is a majority any more, which is the news: every failure
that shared a cause with others has been fixed, and the eleven left
are individual rules rather than one missing mechanism. Expect each to
cost about what it is worth.

The design constraint is fixed: the parser must never perform I/O. The
shape is a caller-supplied map from identifier to content, so the
caller decides what may be read. See
[adr/0003-no-external-entities.md](adr/0003-no-external-entities.md).

Regenerate the list with
`cargo run --release -p oxml-conformance --bin report` rather than
trusting these counts once the code has moved.

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
16,004-node document, it holds 191,957 bytes at peak against
2,277,184 — 92% less, and nearly all of what remains is the
normalised copy of the input.

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

## Releasing 0.0.4 — the ordering matters

The suite ships one version across six crates, and this release
contains a change that makes the order load-bearing.

**oxml 0.0.4 resolves namespace prefixes in XPath name tests**, and an
unbound prefix is now a compile error. The satellites pass expressions
straight through and none of them can supply bindings:

| Crate | Needs |
|---|---|
| `oxml-cli` | a `-n, --ns PREFIX=URI` flag — specified in its `doc/NAMESPACES.md`, not implemented |
| `oxml-wasm` | a second argument on the query methods |
| `oxml-mcp` | an optional `namespaces` argument on `xml_query`, and namespaces reported by `xml_inspect` |

Bump a satellite's dependency without adding its binding mechanism and
namespaced queries become **impossible** in that tool: a
previously-wrong answer turns into an error with no remedy, which is
worse than either.

So for each satellite, **the dependency bump and the binding mechanism
are one change**, not two commits:

1. Merge and publish `oxml` 0.0.4.
2. Per satellite: bump the dependency *and* add its bindings API,
   together.
3. Publish the satellites.

`namespace-uri()` selects on a namespace without naming a prefix and
works in every version, so it is the answer for anyone caught between
the two — and it stays useful afterwards, for when the URI is known and
the prefix is not.

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
