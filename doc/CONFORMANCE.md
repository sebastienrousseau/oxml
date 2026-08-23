<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Conformance

Results against the W3C XML Conformance Test Suite, what the failures
are, and how the numbers are produced.

## Contents

- [Current results](#current-results)
- [How to read these numbers](#how-to-read-these-numbers)
- [What the failures are](#what-the-failures-are)
- [The one document rejected wrongly](#the-one-document-rejected-wrongly)
- [What is unsupported, and why](#what-is-unsupported-and-why)
- [The baseline ratchet](#the-baseline-ratchet)
- [Reproducing](#reproducing)

## Current results

Suite: `xmlts20130923`, 2,585 tests, pinned by SHA-256.

```
overall  2393 pass, 164 fail, 0 panic, 28 unsupported, 0 blocked
         93.6% of 2557 decided (98.9% coverage of 2585)
```

By submission:

| Submission | Pass rate | Decided | Unsupported |
|---|---|---|---|
| japanese | 100.0% | 6 | 6 |
| eduni | 96.7% | 552 | 13 |
| sun | 95.6% | 159 | 0 |
| oasis | 94.2% | 345 | 3 |
| ibm | 93.3% | 1,131 | 5 |
| xmltest | 88.2% | 364 | 1 |

**Zero panics.** No document in the suite makes the parser abort. That
is the number to look at first: a wrong answer is a bug, but a panic on
input from the network is a denial of service.

## How to read these numbers

The pass rate and the coverage figure are always reported together, and
neither means much alone.

- **93.6% of 2,557 decided** — of the tests where the parser gave a
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

164 failures, of which **163 are the parser being too permissive** —
accepting a document the suite says is not well-formed — and **one is
the parser being too strict**.

That asymmetry matters. A parser that wrongly accepts produces a tree
from a document another implementation would reject, so two systems
disagree about a file. A parser that wrongly rejects refuses work it
should have done, which is the more visible failure and the rarer one
here.

| Direction | Count |
|---|---|
| Accepted a document that is not well-formed | 163 |
| Rejected a valid document | 1 |

By submission: ibm 76, xmltest 43, oasis 20, eduni 18, sun 7.

The bulk need the **external DTD subset** — declarations in a separate
file, referenced by `SYSTEM` or `PUBLIC`. oxml parses the internal
subset in full but never fetches an external one, by the same design
that forecloses XXE. A document whose only defect is a violation of a
constraint declared externally is therefore accepted.

Closing that gap means letting a caller supply external subsets from
their own storage, so the parser can validate against them without ever
opening a file itself. That is the intended fix and it is not done.

## The one document rejected wrongly

`eduni/rmt-e2e-50`, and it is a genuine defect rather than a missing
feature:

```
<?xml version="1.1" encoding="iso-8859-1"?>
<!DOCTYPE foo [
<!ELEMENT foo ANY>
<!ATTLIST foo bar CDATA #IMPLIED>
]>
<foo\x85bar="hello"/>
```

Byte `0x85` in ISO-8859-1 is U+0085, NEXT LINE. **XML 1.1 §2.11**
requires a processor to behave as though it had normalised U+0085 and
U+2028 to U+000A before parsing, alongside the XML 1.0 rules for
carriage return. So the document reads `<foo` LF `bar="hello"/>` — the
NEL separates the element name from its attribute, and the document is
valid.

oxml rejects it with `expected a name`, because its whitespace tests
are byte-level (`' '`, `'\t'`, `'\r'`, `'\n'`) and U+0085 is two bytes
in UTF-8. The fix is a line-ending normalisation pass ahead of the
parser, applying the XML 1.1 rules when the declaration names 1.1. It
is not a local change: normalisation rewrites the input, and `parse`
currently borrows the caller's `&str` rather than owning a copy.

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
