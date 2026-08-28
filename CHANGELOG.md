# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Every member of the oxml suite ships the **same version number**. If the
core is at `0.0.X` then so is every satellite, so there is never a
compatibility table to consult. Versions advance in `0.0.1` steps along
the `0.0.x` line; `0.1.0` follows `0.0.999`.

## [Unreleased]

### Added

- **Streaming from a reader.** `Reader::from_reader` and
  `from_reader_with` take any `BufRead`, so a document larger than
  memory can now be read; previously `Reader` took a `&str` and the
  whole document had to be resident. Memory stays bounded regardless
  of document size -- `tests/allocations.rs` reads a 185 KB and a
  1,929 KB document and finds both holding 34,722 bytes.

  Tokens straddling a refill are handled rather than avoided: a
  multi-byte character split across a read is held back until it is
  whole, a `\r\n` pair split across one normalises to a single line
  ending, and a construct that is not yet complete is not scanned at
  all. Speculative scanning was rejected because a half-scanned start
  tag has already interned a name and pushed a namespace scope, so a
  retry would see state the first attempt left behind.

  `tests/stream_from_reader.rs` compares streamed against in-memory
  events over twelve documents at chunk sizes 1, 2, 3, 7, 64 and 8192,
  and requires identical error *kinds and offsets*, not merely
  identical failures.

### Fixed

- **`Reader` read `standalone` from the raw input** rather than from
  the text with line endings normalised, disagreeing with the tree
  parser and with `from_reader`. In XML 1.1 a NEL or U+2028 between
  `standalone` and its `=` is whitespace only after normalisation, so
  `standalone="yes"` went unseen -- and the flag is what withdraws the
  excuse for an entity an unread external subset might have declared.
  A document referencing an undeclared entity therefore parsed
  successfully, substituting empty text for content it had asked for.

## [0.0.6] - 2026-08-26

### Changed

- No change to `oxml` itself. The suite ships one version number
  across all six crates, and this release exists for xmlschema 0.0.6,
  whose W3C conformance pass rate moved from 71.7% to 95.6%.

## [0.0.5] - 2026-08-24

### Added

- **The six `XPath` 1.0 functions that were missing**: `substring-before`,
  `substring-after`, `translate`, `name`, `id` and `lang`. The library
  now implements all 27 the specification defines.
- **`Document::prefix`**, the prefix an interned name was written with,
  and **`Document::element_by_id`**, the element carrying an `ID`-typed
  attribute with a given value.
- **All thirteen `XPath` axes.** `following`, `preceding` and
  `namespace` were absent; the roadmap called `XPath` complete on the
  strength of the function library alone.
- **`NodeKind::Namespace` and `Document::namespace_nodes`.** Namespace
  declarations were resolved and discarded, so there was nothing for
  `namespace::` to return. One node exists per declaration, not per
  element it is in scope for -- the axis walks ancestors and applies
  shadowing. `xml` is bound by specification, so the root element
  carries a node for it, which is one extra node per document: a
  document's `len()` is one higher than before.
- **Three benchmark groups**: `encoding` (the zero-copy UTF-8 path
  against the three that must allocate), `tree` (traversal, subtree
  text, attribute lookup) and `entities` (expansion against a control
  with the same output and no entities). 7 benchmarks to 19.
- **An `external_entities` example.** `parse_with_external` counted as
  covered only because `parse_with` delegates to it internally, so the
  43/43 figure was true and meant nothing.
- **`scripts/check-doc-links.py`**, gated in CI: every markdown link
  and in-page anchor must resolve.

### Fixed

- **An unknown function is now a compile error.** It used to compile and
  evaluate to an empty node-set, so `substring-bfore(...)` -- or any of
  the six functions above -- returned "no matches" rather than an error.
  A caller could not distinguish a misspelled function, an unimplemented
  one, and a document that genuinely had no match.
- **The wrong number of arguments is now a compile error.** Arity was
  not checked, so a missing argument read as the empty string and the
  call returned something plausible: `starts-with("abc")` answered
  **true**, because every string starts with the empty string, and
  `translate("abc", "ab")` returned `"c"`, having silently deleted the
  characters it had no replacement for.
- **`name()` reports the prefix.** Names are interned by expanded name,
  which discards the prefix, and namespace declarations are not retained
  as attributes, so there was nothing left to rebuild a QName from.
  Interned names now carry the prefix they were written with.
- **Attributes are compared for duplication by expanded name, not by
  interned id.** Making ids prefix-sensitive would otherwise have
  admitted `p:x` and `q:x` with both prefixes bound to one namespace,
  which Namespaces in XML forbids.

### Documentation

- **Four broken anchors**, all contents entries pointing at renamed
  headings -- including one titled "Entity expansion is not supported"
  for a section explaining that expansion happens, boundedly.
- **Each README had broken links the other did not.** They are
  byte-identical but sit at different depths, so the root copy pointed
  at examples under `crates/oxml/` and the crate copy -- the one
  docs.rs renders as the crate documentation -- pointed at a
  `SECURITY.md` and `CHANGELOG.md` that exist only at the repository
  root. Both now use absolute URLs.
- **The README published six absolute timings** while
  `doc/BENCHMARKS.md` states that no figure is published without its
  machine, toolchain, load average and confidence interval. The table
  now says what each benchmark isolates and publishes no number. It
  also documented `parse/deep_500`, renamed to `deep_max` when 500
  levels exceeded `MAX_DEPTH`, and omitted `eval_numeric_predicate`.
- **`doc/CONFORMANCE.md` stated 98.6% in one paragraph and 93.6% in
  another**, and carried a per-submission table from the 93.6% era that
  understated `xmltest` by eight points. Conformance figures in `doc/`
  are now pinned by a test against the harness output.
- Stale counts corrected throughout: 25 functions (21 listed, 21
  implemented), 244 tests, 16 doctests, 97.4% coverage.
- `#![deny(missing_docs)]` rather than `warn`, as the house style
  specifies.

### Changed

- `NameId` equality now means the names were written identically,
  prefix included -- not that they denote the same expanded name.
  Compare `Document::name` values where that is the question.

## [0.0.4] - 2026-08-24

### Added

- **`parse_with_external` and the `ExternalSource` trait.** oxml still
  performs no I/O; a caller supplies external entity and subset content
  and the parser asks for it by identifier. With content available, the
  rules only that content can settle are checked -- a text declaration
  must be well formed, name an encoding, omit `standalone`, and not
  declare a version later than the document's.
- **`XPath::compile_with_namespaces`.** A prefix in an expression now
  resolves against bindings supplied with the query.
- **`Document::name` and `NameId`.** Element and attribute names are
  interned; `NodeKind::Element` and `Attribute::name` carry a handle.
- **`Display for ErrorKind`**, so a caller drawing a caret can print the
  message without repeating the byte offset.

### Fixed

- **Line endings were not normalised.** `<a>x\r\ny</a>` returned
  `"x\r\ny"` where the specification requires `"x\ny"` -- every document
  written on Windows. XML 1.1's NEL and LINE SEPARATOR are handled too.
- **Attribute values were not normalised.** An attribute wrapped across
  two lines carried the newline and the next line's indentation.
- **XPath name tests ignored namespace prefixes.** `//x:item` selected
  every `item` regardless of namespace -- a wrong answer with no error
  attached. An unbound prefix is now a compile error, and an unprefixed
  name test matches only nodes in no namespace, as XPath 1.0 requires.
- **`local-name()` and `namespace-uri()` returned `""` for every
  attribute node.**
- **`Error::line_column` could panic** on an offset inside a multi-byte
  character -- a panic in the error-reporting path.
- Around twenty further well-formedness rules that were parsed and not
  enforced: `]]>` in text, `<` in attribute values, the XML declaration's
  own version number, reserved namespaces as the default, colons in
  entity, notation and processing-instruction names, `NDataDecl` syntax,
  `<!ATTLIST>` default values, conditional section keywords, and the
  entity constraints `WFC: PEs in Internal Subset`,
  `WFC: No External Entity References` and `WFC: Parsed Entity`.

### Changed

- **Allocations: 4.13 per node to 1.13.** Child and attribute lists are
  `(start, len)` ranges into shared vectors, names are interned and
  borrow the input until interning, and the arena is sized up front.
- **Conformance: 93.6% to 98.6%** of decided W3C tests (2,520 of 2,557),
  with 98.9% of the 2,585-test suite reaching a decision and **zero
  panics**. No document in the suite is wrongly rejected.

### Breaking

- `Attribute::name` is a `NameId` rather than an `ExpandedName`; resolve
  it with `Document::name`.
- `NodeKind::Element` carries `NameId` and `(u32, u32)`.
- A prefixed XPath name test now requires a binding, and an unprefixed
  one no longer matches namespaced nodes.

## [0.0.3] - 2026-08-22

### Fixed

- **`substring()` returned the wrong characters when the start index was
  below 1.** The implementation clamped the start to position 1 and then
  took `length` characters, which is a different function from the one
  the specification defines. XPath keeps every character whose 1-based
  position `p` satisfies `p >= round(start)` and
  `p < round(start) + round(length)`, so positions at or below zero
  still consume part of the window. `substring("12345", 0, 3)` — an
  example given in the specification itself — returned `"123"` instead
  of `"12"`. All four of the specification's worked examples now match.

- **`round()` broke ties in the wrong direction.** It used Rust's
  `f64::round`, which rounds half away from zero, where XPath requires
  the tie to go towards positive infinity. `round(-1.5)` returned `-2`
  instead of `-1`.

- **`local-name()` and `namespace-uri()` ignored their argument.** Both
  read the context node unconditionally, so `local-name(//x)` described
  whatever node the expression was evaluated from — usually the document
  node, which has no name, so the answer was always the empty string.
  They now use the first node of the argument node-set when one is
  given, and the context node only when it is not.

### Security

- **A deeply nested document no longer aborts the process.** Parsing
  descends one stack frame per open element, so a sufficiently nested
  document exhausted the stack. A stack overflow aborts rather than
  unwinding, so no caller could catch it, and the depth at which it
  happened depended on the thread's stack size — around 800 on a main
  thread, around 500 on a test thread. Every front end of this crate
  reads documents it did not write, which made this a denial of service
  rather than a curiosity. Nesting is now bounded by
  [`MAX_DEPTH`](https://docs.rs/oxml/latest/oxml/constant.MAX_DEPTH.html)
  (256) and exceeding it is an ordinary parse error.

- **The same bound applies to XPath expressions.** `((((...))))` nested
  deeply enough overflowed the expression parser identically, and an
  expression is untrusted input in every front end: the CLI takes one
  from a shell, the MCP server from a model, the WASM bindings from
  JavaScript.

### Added

- **`processing-instruction()` node test**, with the optional literal
  target argument (`processing-instruction('render')`). The tree already
  recorded processing instructions and the evaluator could already take
  their string value; only the node test was missing, so they could not
  be selected at all. This completes XPath 1.0's four node tests.

- **`MAX_DEPTH`**, the public nesting bound described above.

### Changed

- Test coverage raised from 78% to 96% of regions (97% of lines), which
  is what surfaced every defect above.

## [0.0.2] - 2026-08-22

### Added

- **A pure-Rust XML parser.** Single forward pass, no backtracking, no
  intermediate token vector — the tree is built as the scan proceeds.
  Namespaces resolve by URI, entities are restricted to the five
  predefined ones plus numeric character references.
- **An arena-based document tree.** Nodes are index-addressed handles,
  so parent links need no `Rc`, no `RefCell`, and no `unsafe`.
  Attributes are real nodes: reachable from `attribute::`, absent from
  `child::`, exactly as XPath models them.
- **An XPath 1.0 engine.** Ten axes, node tests, predicates, the four
  value types with their specified conversions, and 25 functions.
  Compilation is separate from evaluation so an expression parsed once
  can run against many documents.
- **Benchmarks** for parsing (wide, deep and attribute-heavy shapes)
  and XPath (compilation and evaluation timed separately, because a
  caller reusing a compiled query pays only the second).

### Notes

Two defects were found by the project's own tests and benchmarks
before the first release, and are worth recording because both were
silent:

- The attribute axis originally yielded the *owning element* rather
  than the attribute, so `string(//book/@lang)` returned the book's
  text instead of `"en"`. Attributes are now arena nodes.
- Per-step deduplication used a linear `contains` scan, making it
  O(n²). `//title` over a 2,000-element document took **10.8 ms**;
  sorting instead brought it to **0.49 ms**, a 22× improvement with no
  change in results.

[0.0.2]: https://github.com/sebastienrousseau/oxml/releases/tag/v0.0.2
[0.0.3]: https://github.com/sebastienrousseau/oxml/releases/tag/v0.0.3
