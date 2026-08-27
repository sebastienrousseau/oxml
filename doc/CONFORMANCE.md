<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Conformance

Results against the W3C XML Conformance Test Suite, what the failures
are, and how the numbers are produced.

## Contents

- [Current results](#current-results)
- [How to read these numbers](#how-to-read-these-numbers)
- [What the failures are](#what-the-failures-are)
- [The document that was rejected wrongly, and is not any
  more](#the-document-that-was-rejected-wrongly-and-is-not-any-more)
- [What is unsupported, and why](#what-is-unsupported-and-why)
- [The baseline ratchet](#the-baseline-ratchet)
- [Reproducing](#reproducing)

## Current results

Suite: `xmlts20130923`, 2,585 tests, pinned by SHA-256.

```
overall  2551 pass, 6 fail, 0 panic, 28 unsupported, 0 blocked
         99.8% of 2557 decided (98.9% coverage of 2585)
```

By submission:

| Submission | Pass rate | Decided | Unsupported |
|---|---|---|---|
| japanese | 100.0% | 6 | 6 |
| oasis | 100.0% | 345 | 3 |
| eduni | 99.6% | 552 | 13 |
| ibm | 99.7% | 1,131 | 5 |
| sun | 99.4% | 159 | 0 |
| xmltest | 100.0% | 364 | 1 |

**Zero panics.** No document in the suite makes the parser abort. That
is the number to look at first: a wrong answer is a bug, but a panic on
input from the network is a denial of service.

## How to read these numbers

The pass rate and the coverage figure are always reported together, and
neither means much alone.

- **99.8% of 2,557 decided** — of the tests where the parser gave a
  definite answer, this many agreed with the suite.
- **98.9% coverage of 2,585** — this many of the suite's tests produced
  a definite answer at all.

A pass rate on a thin denominator is the easiest number in software to
flatter. Skip every hard test and 100% is trivial. The coverage figure
is the denominator made visible, which is why it is never omitted.

Twice during development a way was found to reclassify failures that
would have pushed the headline past 95%. Both were reverted, because
both raised the number without changing what the parser does.

## What the failures are

6 failures, and **every one of them is the parser being too
permissive** — accepting a document the suite says is not well-formed.
There is no longer a document the parser wrongly rejects.

The previous count was 37. The thirteen that went were a single cause:
an entity's replacement text being substituted as characters where
XML 1.0 §4.4.2 requires it to be *included* — parsed as content. They
are listed in `crates/oxml/tests/entity_markup.rs`, which keeps them
fixed.

What remains needs content oxml never reads — and, in every case,
rules *about* that content rather than the content itself. The caller
already supplies it: the conformance runner hands the parser every
file sitting beside the document, which is how the two `sun`
conditional-section tests came to pass.

| What the failure needs | Count |
|---|---|
| Standalone declarations against external content | 2 |
| External identifier and entity-reference rules | 2 |
| An external DTD (`eduni/rmt-*`) | 2 |

Three submissions — `japanese`, `oasis` and `xmltest` — now pass every
test they decide, and `ibm` is at 99.7% of 1,131.

No group is now a majority, which is itself the news: the failures
that shared a cause have been fixed, and what is left is individual
rules rather than one missing mechanism.

Derived by reading the 37 rather than estimating: the thirteen above
were confirmed one at a time against the specification, and the
remaining 24 grouped by the file each one turns on. Regenerate with
`cargo run --release -p oxml-conformance --bin report` rather than
trusting this table once the code has moved.

That table has been wrong twice, in the same direction both times: I
estimated what a group needed instead of counting it. The estimate
before this one put ~35 failures on parameter entity expansion;
implementing it moved **one**.

Counted by what the failure *needs* rather than by where the `DOCTYPE`
points, which is what the earlier table did and why it read as more
tractable than it was. A document can have a purely internal subset and
still turn on the content of a separate file.

That table is coarser than it looks, and worth qualifying now that the
easy group is gone. A document can have a purely internal subset and
still fail on something external, because an entity it declares points
at a separate file: of the 51, roughly 35 turn on the **content** of an
external parsed entity — its text declaration, its version number, its
standalone declaration. oxml never reads those files, so those are the
external-subset problem wearing a different hat.

**The largest genuinely-internal group left is entity replacement text
being substituted rather than parsed.** `<!ENTITY e "<foo/>">`
referenced from content should produce an *element*; oxml produces
text. That is a real semantic gap, not a missing check, and it is the
next substantial piece of work in this area.

That categorisation is worth doing before assuming otherwise. This
document once said "the bulk need the external DTD subset", which was a
guess and was wrong: the split was 84 internal-only against 63
external. Counting instead of guessing found 16 failures with no
`DOCTYPE` at all and a further set needing only declarations already
parsed.

**34 have since been fixed, taking the count from 163 to 129 — and the
external-subset group has not moved once. 63 then, 63 now.** Every
single fix was a rule the parser already had the information to
enforce; not one of them needed a feature.

That asymmetry matters. A parser that wrongly accepts produces a tree
from a document another implementation would reject, so two systems
disagree about a file. A parser that wrongly rejects refuses work it
should have done, which is the more visible failure and the rarer one
here.

| Direction | Count |
|---|---|
| Accepted a document that is not well-formed | 149 |
| Rejected a valid document | 0 |

63 of them — unchanged throughout — need the **external DTD
  subset** — declarations in a
separate file, referenced by `SYSTEM` or `PUBLIC`. oxml parses the
internal subset in full but never fetches an external one, by the same
design that forecloses XXE. A document whose only defect is a violation
of a constraint declared externally is therefore accepted.

Closing that gap means letting a caller supply external subsets from
their own storage, so the parser can validate against them without ever
opening a file itself. That is the intended fix and it is not done.

## The document that was rejected wrongly, and is not any more

`eduni/rmt-e2e-50` was the single document in the suite that oxml
refused and should not have:

```
<?xml version="1.1" encoding="iso-8859-1"?>
<!DOCTYPE foo [ … ]>
<foo\x85bar="hello"/>
```

Byte `0x85` in ISO-8859-1 is U+0085, NEXT LINE, which **XML 1.1
§2.11** requires a processor to normalise to `\n` before parsing. So
the document reads `<foo` LF `bar="hello"/>` — the NEL separates the
element name from its attribute — and it is valid.

Fixed by normalising line endings ahead of the parser. Writing the
cases out first showed that **XML 1.0 normalisation was missing too**:
`<a>x\r\ny</a>` returned `"x\r\ny"` where the specification requires
`"x\ny"`, which affects every document written on Windows. The
conformance suite never caught that, because it tests whether a
document is *accepted*, not what its text turns out to be.

See [the design note](design/xml-1-1-line-endings.md).

## What is unsupported, and why

28 tests, excluded from the pass rate and counted in coverage:

| Reason | Count | Why |
|---|---|---|
| `namespace-processing-off` | 14 | The suite asks for a parser with namespace handling disabled. oxml always resolves namespaces. |
| `namespaces-1.1` | 8 | Namespaces in XML 1.1, which adds prefix undeclaration. |
| `unsupported-encoding` | 6 | Encodings outside UTF-8, UTF-16 and ISO-8859-1 — mostly Shift_JIS and EUC-JP. |

These are marked unsupported rather than failed on purpose. Collapsing
the two either flatters the score or buries real defects, depending on
which way you collapse it.

## The baseline ratchet

`conformance/baselines/w3c-xml.tsv` records the expected outcome of
every one of the 2,585 tests. The run fails on a regression **and** on
an unreviewed improvement.

Failing on improvement is not pedantry. When a batch of tests starts
passing, the usual cause is not that the parser got better — it is that
the runner stopped running them properly. That happened here: a loader
keyed on the wrong element silently dropped all 159 Sun tests and
reported a confident number for the remaining 2,426. It was caught by
an assertion on the test count, not by anyone reading the output.

## Reproducing

```bash
cargo run -p oxml-conformance --bin download   # 15 MB, hash-pinned
cargo test -p oxml-conformance --release       # the ratchet
cargo run -p oxml-conformance --bin report --release
```

The downloader verifies the archive against a SHA-256 pinned in the
source. That digest is computed by an implementation in this repository
which is checked against the published FIPS 180-2 vectors, including
the block-boundary cases at 55, 56, 63, 64 and 65 bytes — a digest that
is never checked against a known answer produces 64 plausible hex
characters for any input, and the pin it backs means nothing.
