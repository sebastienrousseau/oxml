# oxml-conformance

The [W3C XML Conformance Test Suite][suite], run against `oxml` and
ratcheted against a committed baseline.

## Results

Suite release **`xmlts20130923`**, 2,585 tests.

| | |
|---|---|
| **Pass rate** | **80.3%** (1,519 of 1,892 decided) |
| **Coverage** | **73.2%** (1,892 of 2,585) — see the ceiling below |
| Panics | **0** |

Both numbers are given because either alone is misleading. A pass rate
without a denominator can be raised by skipping the hard tests; coverage
without a pass rate says nothing about correctness.

### The coverage ceiling is 86.6%, not 100%

Two groups can never be decided by *any* conforming parser:

- **309 tests marked `EDITION="1 2 3 4"`.** The suite ships
  complementary pairs — the same name is not-well-formed under editions
  1–4 and well-formed under the 5th, which relaxed `NameStartChar`. A
  parser must pick one edition; scoring against both is incoherent.
  `oxml` targets the **5th edition**.
- **33 tests marked `TYPE="error"`**, which the suite's own DTD says are
  optional to report, so either outcome conforms.

That is 346 tests, capping coverage over the full suite at **86.6%**.
Quoting a coverage figure without this is how a runner appears to be
hiding tests it is in fact entitled to skip.

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
| XML 1.0 5th edition only | 375 |
| XML 1.1 | 238 |
| Non-UTF-8 bytes | 81 |
| `TYPE="error"` (optional per the suite's DTD) | 33 |
| Non-UTF-8 declared encoding | 17 |
| Namespaces 1.1 | 6 |
| `NAMESPACE="no"` | 4 |

### What the failures are

| Kind | Count |
|---|---|
| Accepted a document that is not well-formed | 780 |
| Rejected a valid document | 126 |
| Invalid document treated as not well-formed | 48 |

**759 of the 780 wrongly-accepted documents contain a `<!DOCTYPE`.**
`oxml` skips the DTD rather than parsing it, so it cannot detect
malformed DTD syntax or undeclared entities — and well-formedness
constraints inside the DTD bind every parser, validating or not.
Implementing internal-subset DTD *syntax* checking would move the pass
rate to roughly 89% without any validation work. That is the single
highest-value change this suite identifies.

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
