# oxml-conformance

The [W3C XML Conformance Test Suite][suite], run against `oxml` and
ratcheted against a committed baseline.

## Results

Suite release **`xmlts20130923`**, 2,585 tests.

| | |
|---|---|
| **Pass rate** | **91.5%** (2,339 of 2,557 decided) |
| **Coverage** | **98.9%** (2,557 of 2,585) |
| Panics | **0** |

Both numbers are given because either alone is misleading. A pass rate
without a denominator can be raised by skipping the hard tests; coverage
without a pass rate says nothing about correctness.

### How 98.9% was reached, and why it took a feature rather than a metric

Coverage was stuck at 85.8% against an apparent ceiling of 86.6%,
because two groups looked undecidable:

- **309 tests marked `EDITION="1 2 3 4"`.** The suite ships
  complementary pairs — the same name is not-well-formed under editions
  1–4 and well-formed under the 5th, which replaced Appendix B's
  enumerated character classes with broad ranges. The two **disagree**;
  they are not a superset relation. A parser fixed to one edition cannot
  be scored against the other's tests at all.
- **33 tests marked `TYPE="error"`**, which the suite's own DTD says a
  parser *may* report.

Neither is now skipped, and neither was resolved by redefining the
denominator:

- `oxml` implements **both editions**, selected by `Limits::edition`,
  with the 5th as the default. Appendix B's `BaseChar`, `Ideographic`,
  `CombiningChar`, `Digit` and `Extender` tables are transcribed in
  `names4e.rs`. Their correctness is not asserted — it is measured, by
  the 309 tests that exist to distinguish the editions.
- `TYPE="error"` tests are scored as passes, because **both outcomes
  conform**. What must not happen is a panic, and that is checked
  separately. Leaving them undecided understated coverage for a
  condition on which the specification is explicitly permissive.

The 28 still unsupported are Namespaces 1.1 (prefix undeclaration),
`NAMESPACE="no"`, and encodings outside UTF-8/UTF-16/Latin-1.

### By submission

| Submission | Pass rate | Decided | Unsupported |
|---|---|---|---|
| sun | 64.0% | 150 | 9 |
| xmltest (James Clark) | 62.4% | 356 | 9 |
| eduni (Edinburgh) | 60.8% | 102 | 463 |
| oasis | 56.2% | 313 | 35 |
| japanese (Fuji Xerox) | 50.0% | 2 | 10 |
| ibm | 35.2% | 908 | 228 |

### Why tests are skipped

Every exclusion is a feature `oxml` does not claim. None of them is a
judgement about whether a test is fair.

| Reason | Count |
|---|---|
| XML 1.0 editions 1–4 only (not applicable — see the ceiling) | 309 |
| `TYPE="error"` (optional per the suite's DTD) | 33 |
| Namespaces 1.1 | 8 |
| `NAMESPACE="no"` | 9 |
| Unsupported encoding | 8 |

### What the failures are

| Kind | Count |
|---|---|
| Accepted a document that is not well-formed | 217 |
| Rejected a valid document | 1 |

Effectively every remaining failure is a document accepted that should
have been rejected. The wrongly-*rejected* class is gone: only one
remains, down from 126.

By spec section, the largest clusters are 2.8 (prolog and document type
declaration), 3.4 (conditional sections), 3.2.1 (element content
models) and 2.3 (common syntactic constructs). These are the
well-formedness constraints the DTD parser does not yet enforce — it
checks the *grammar* of a declaration but not, for example, that a
conditional section keyword is `INCLUDE` or `IGNORE`.

## Running it

```sh
cargo run -p oxml-conformance --bin download   # ~641 KB, verified
cargo test -p oxml-conformance                 # ratchet against baseline
cargo run -p oxml-conformance --bin report     # human-readable summary
```

To accept a change deliberately:

```sh
OXML_UPDATE_BASELINE=1 cargo test -p oxml-conformance
```

An **improvement fails the ratchet too**. That is intentional: a pass
rate that drifts upward because tests began being skipped rather than
passing is indistinguishable from progress unless every change to the
number is reviewed.

## Notes on the suite itself

The suite has quirks that shape this harness, and all of them were
verified against the artefact rather than taken on trust:

- **www.w3.org is behind Cloudflare.** A `curl` without a browser
  User-Agent returns a 5,850-byte HTML challenge page with HTTP 200 —
  not the 641,522-byte tarball. The download binary sets a UA and
  verifies a pinned SHA-256, so a challenge page fails loudly instead of
  producing an empty directory and a zero-test "pass".

- **It is downloaded, never vendored.** The tarball ships no LICENSE
  file — the terms live only in the [FAQ][faq] — and James Clark's
  `xmltest/` portion forbids redistribution in modified form.

- **2,586 vs 2,585.** Both figures circulate. There are 2,586 `<TEST`
  occurrences and 2,585 tests; one sits inside an XML comment in
  `ibm/xml-1.1/ibm_not-wf.xml`. Parsing the manifests rather than
  pattern-matching them gets this right without special-casing.

- **Sun's manifests have no root element.** `sun/sun-{valid,invalid,
  not-wf,error}.xml` are bare sequences of `<TEST>`, meant to be pulled
  into `xmlconf.xml` by external entity reference, and so are not
  well-formed documents. A loader keyed on `<TESTCASES>` silently drops
  all 159 of them — this one did, at first, and reported 2,426 tests.
  The count assertion is what caught it.

- **Manifests mix quote styles.** OASIS uses single quotes throughout,
  which a regex-based reader gets wrong for all 348 of its tests.

- **The per-collection manifests are read directly**, not through
  `xmlconf.xml`. That top-level file includes the others by *external
  entity reference*, which `oxml` deliberately does not resolve, and it
  carries a known defect: the `eduni-misc` entity is wrapped in
  `xml:base="eduni/namespaces/misc/"`, a directory that does not exist,
  so a runner honouring `xml:base` correctly loses all nine of the 2013
  release's new tests.

- **Version matters.** libxml2 and Expat both still pin
  `xmlts20080827`, the 2008 release, which disagrees with this one about
  three tests' expected outcomes. "Passes the W3C suite" means nothing
  without a release.

[suite]: https://www.w3.org/XML/Test/
[faq]: https://www.w3.org/XML/Test/faq.html
