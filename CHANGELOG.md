# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Every member of the oxml suite ships the **same version number**. If the
core is at `0.0.X` then so is every satellite, so there is never a
compatibility table to consult. Versions advance in `0.0.1` steps along
the `0.0.x` line; `0.1.0` follows `0.0.999`.

## [0.0.11] - Unreleased

### Changed

- **Breaking: the MSRV is now Rust 1.89.0** (was 1.86.0), one floor
  shared with the maintainer's other Rust crates.
- **The MSRV job checks its toolchain first.** It already set
  `RUSTUP_TOOLCHAIN`; it now also asserts `rustc --version` before
  building.
- **`as_chunks` replaces two suppressed lints.** The UTF-16 decoder and
  the conformance SHA-256 used `chunks_exact` under
  `#[allow(clippy::chunks_exact_to_as_chunks)]` because `as_chunks`
  (1.88) was above the floor; both now use it.
- **Lints the new floor enables**: let-chains for nested `if let`s and
  `is_multiple_of`, applied as clippy suggests. Behaviour is unchanged.

### Added

- **`Document::try_to_xml` and `SerialiseError`.** A built document can
  need XML 1.1 (a control character, or a prefix unbound with
  `xmlns:p=""`) and also hold a comment or PI with a character 1.1
  does not keep literally: `U+0085` and `U+2028` are line endings
  there and the rest of `U+007F..U+009F` is refused. A body has no
  reference syntax, so no version writes that document faithfully;
  `to_xml` wrote it as 1.1 and the comment came back changed.
  `try_to_xml` returns `SerialiseError::VersionConflict` naming the
  node and the character, and otherwise exactly what `to_xml` returns.
  A parsed document always succeeds. Found by the `mutate` fuzz
  target, which now uses `try_to_xml` and accepts a refusal only when
  `to_xml` of the same document is not a fixed point.

### Fixed

- **A detached subtree no longer decides the XML version.** The
  serialiser declared 1.1 for a control character anywhere in the
  arena, including a subtree `detach` had taken out of the document.
  That subtree is not written, so the output reparsed without the
  declaration and serialising it again dropped it: not a fixed point.
  Only the nodes the writer visits are considered now. Found by the
  `mutate` fuzz target.
- **U+2028 is written as `&#8232;`.** XML 1.1 reads LINE SEPARATOR as
  a line ending, so written literally into a document that needs 1.1
  it came back as a newline in text and as a space in an attribute
  value or namespace URI. The reference reads back as the character in
  both versions, as `&#133;` already did for U+0085.
- **`append_comment` and `append_pi` refuse a carriage return.** A
  comment or PI body is written verbatim, and a literal `\r` reads back
  as `\n` in every XML version, so `<!--a\rb-->` did not survive its
  own round trip. They now return `NodeError::InvalidText`; character
  data still takes `\r`, written as `&#13;`.
- **The streaming reader ignores a declaration that never closes.**
  `Reader::from_reader` read the `encoding` pseudo-attribute of a
  `<?xml` with no `?>` anywhere in the input, and refused the document
  for an unsupported or illegal name. `encoding::decode` and the tree
  parser treat such a declaration as absent, decode as UTF-8 and
  report the unterminated declaration, and the streaming reader now
  does the same. Found by the `stream_chunked` fuzz target.
- **The streaming reader decides the encoding as `encoding::decode`
  does, with the same functions.** It read the declaration from text
  in which every byte that was not UTF-8 had become two, so an error
  in the `encoding` pseudo-attribute was reported one byte later per
  such byte; and under a UTF-16 mark it refused a name `decode`
  leaves alone. `decode` in turn checked a UTF-8 mark against the
  declaration only when every later byte was UTF-8, so a contradicting
  declaration let the document through as Latin-1 when one was not:
  it is now refused with `MalformedEncoding` at 0, as it already was
  otherwise. Found by the `stream_chunked` fuzz target.

## [0.0.10] - 2026-10-05

### Security

- **`XPath` expressions are bounded.** `Limits::max_xpath_depth` and
  `Limits::max_xpath_operators` were documented, rendered into the README
  table, and never read: a 100 KB expression of unary minuses aborted the
  process inside `XPath::compile`, and a 600 KB `1+1+…` chain aborted during
  evaluation. `XPath::compile_with_limits` and
  `compile_with_namespaces_and_limits` apply both; `compile` uses the
  defaults. Unary minus goes through the depth guard, and left-associative
  chains are stored flat (`Expr::Chain`) and folded iteratively, so no
  expression within budget recurses proportionally to its length in parse,
  evaluate, drop, clone, `Debug` or `PartialEq`. Six tests on a 2 MiB thread.
- **The entity-validation cache no longer clones the namespace stack per
  reference.** The key was `(name, Vec<(prefix, uri)>)`, so 20,000 references
  under 800 in-scope bindings took 711 ms and nothing in `Limits` bounded
  it. The key is now `(name, scope id)`, where the scope id identifies the
  binding stack in O(1): 7.7 ms. The DTD, namespace stack and cache are
  moved into the validating sub-parser and back instead of cloned: 4,000
  × 2 KB entities went from 2.7 s to 37 ms.
- **The DTD parser has limits.** `max_name_length` applies to declared
  names, `max_attribute_size` to `ATTLIST` defaults, the new
  `max_pe_expansion` to parameter-entity replacement text (a width bomb of
  100 × 100 × 1 KB is refused in 20 ms) and the new `max_dtd_bytes` to the
  size of the subsets read. Two new error kinds, `DtdTooLarge` and
  `ConstructTooLarge`.
- **The streaming reader bounds what it buffers.** A construct is pulled
  into memory until it is whole before any scan could apply a limit, so
  `<a>` followed by an endless stream of text, or `<!--` never closed, grew
  until memory ran out. The new `max_construct_bytes` (64 MiB by default,
  1 MiB under `strict()`) fails the read instead, and the reader resumes
  the delimiter search from a watermark rather than rescanning from the
  construct start on every 8 KB pull: a 16 MB text node went from 2.7 s
  to 0.11 s. `max_nodes` is now enforced by the reader at the same offset
  as the tree parser.
- **The duplicate-attribute check is no longer quadratic.** Past sixteen
  attributes the expanded names go into a set, so an element with 1,000
  attributes parses at 27 MB/s instead of 5 MB/s, the shape of
  RUSTSEC-2026-0194.
- **A processing instruction cannot impersonate the XML declaration.** The
  version, standalone and encoding sniffers accepted anything starting
  with `<?xml`, so `<?xml-stylesheet href="version='1.1'"?>` switched a
  document to 1.1 rules and `encoding='iso-8859-1'` inside it transcoded
  the whole document. One `xml_declaration` finder requires whitespace
  after `<?xml`.
- **Arena pre-sizing is capped** by `max_nodes` and a fixed ceiling, so
  4 MB of `<` bytes no longer reserves 450 MB before the first limit is
  consulted.

### Added

- **A mutable tree you can trust.** `Document::declare_namespace`,
  `insert_at`, `set_text`, `append_comment`, `append_pi`, `detach`,
  `text_content`, `attribute_ns`, `descendants_of`, `nodes`,
  `document_order`, `order_key`, `is_order_dirty`, `compact`, and
  `Builder::child_ns`. `write_xml_to` serialises to any `std::io::Write`.
  `NodeError` gains `NotAContainer`, `NotAChild`, `NotRemovable`,
  `InvalidName` and `InvalidText`. ADR 0008 records the design.
- **`Document::range`**: the byte range a parsed element, text node,
  comment or processing instruction came from, for editors and linters
  (oxml-lsp). `None` for nodes the mutation API created.
- **`XPath` 1.0 as specified.** Filter expressions (`(//a)[1]`,
  `id('x')/b`), variable references (`$x`) through `EvalContext` and
  `XPath::evaluate_with`, `Display` for `Expr` and `XPath` rendering
  canonical unabbreviated `XPath`, and a function enum resolved at compile
  time.
- **Release engineering.** crates.io Trusted Publishing replaces the
  stored registry token; the release workflow verifies the tag's signature
  against `.github/allowed_signers`, requires `doc/release-notes/<ver>.md`,
  attaches the `.crate`, a CycloneDX SBOM and SHA-256 sums with SLSA
  provenance, and composes the release page in the family format.
  `Cargo.lock` is committed and every CI build is `--locked`; the fuzz
  corpus persists between runs; `cargo-audit`, `cargo-fuzz` and
  `rust-code-analysis-cli` are installed at pinned versions; a
  `cargo-semver-checks` report runs on every pull request; `CITATION.cff`,
  `CODEOWNERS`, issue and pull-request templates and `.compliance.yml`
  declaring L3.
- **Complexity ratchet.** `scripts/check-complexity.py` measures every
  function in `crates/oxml/src` with `rust-code-analysis-cli` against
  per-function ceilings (cyclomatic 10, cognitive 15, Halstead difficulty
  30, 60 lines) and compares the offenders with the committed
  `complexity-baseline.tsv`. CI and `scripts/gate.sh` fail on a new
  offender or a worse one; `--update` can only record improvements. The
  backlog went from 65 functions to under 60 in this release.
- **Ecosystem release cycle v0.0.10.** Begin next iteration cycle across the
  entire lockstep suite.

### Changed

- **`XPath` predicates apply per context node.** `//a[1]` returns the
  first `a` child of every parent, as XPath 1.0 §2.4 requires; it used to
  return one node. Reverse axes count proximity position from the context
  node. `<`, `<=`, `>`, `>=` and `!=` on node-sets are existential.
  `number()` follows the XPath grammar (`1e5`, `+1`, `Infinity` are NaN),
  `sum()` is NaN when any node is not a number, numeric equality is IEEE
  equality, `round()` handles 0.49999999999999994 and negative zero,
  `normalize-space()` splits only on XML whitespace. `following::` and
  `preceding::` are tree walks instead of arena scans. Node-sets follow
  document order after a document is mutated.
- **`Expr` has new variants** (`Chain`, `Filter`, `PathFrom`, `Variable`).
  Code matching on `Expr` exhaustively needs new arms.
- **The mutation API validates its input.** A child appended to a text
  node, a second element under the document root, an invalid name, text
  with a character XML forbids, and removing the document root are errors.
  Namespaced names created through the API are declared on serialisation.
  Appending children is amortised O(1) in retained memory (10,000 children
  retained 538 MB, now 1.1 MB); replacing an attribute value no longer
  retains every previous value. `Document::len` counts live nodes.
- **The serialiser declares XML 1.1 only when it must.** A 1.0 document
  containing U+0080..U+009F literally was re-emitted as `version="1.1"`,
  which 1.0-only consumers reject; those characters are now written as
  references under 1.0, and only C0 controls trigger 1.1.
- **`Limits::max_name_length` default is 8,192** (was 1,000): the W3C
  suite's `ibm85v01` and `ibm87v01` carry a processing-instruction target
  that enumerates every legal character, and the limit now applies inside
  the DTD too.
- **Duplicate namespace declarations are rejected.** `<a xmlns:p="u1"
  xmlns:p="u2"/>` and `<a xmlns="u1" xmlns="u2"/>` violate `WFC: Unique
  Att Spec` and were accepted with the last declaration winning.
- **Documentation migration.** Migrated website and rendered manual to
  `oxmllib.com` (`oxmllib.github.io`) using the Lucid SSG theme.
- **Documentation corrected.** The docs.rs crate doc said custom entities
  are never expanded; `SECURITY.md` said XXE declarations are rejected and
  recommended a bigger stack instead of `max_depth`; the README said
  serialisation and mutation did not exist and listed the five shipped
  satellites as planned; `conformance/README.md` published 93.6% and a
  163-failure table three releases after the figure reached 100%, and is
  now covered by the drift test. All rewritten from the code.

### Fixed

- `remove(attribute)` followed by `remove(element)` freed the attribute's
  slot twice; the next allocation returned a `NodeId` that did not resolve
  and the node vanished from `to_xml()`. Slots are freed once and only by
  the path that unlinks them.
- Namespace nodes of a removed element were never freed.
- `set_attribute` and `remove_attribute` compared attribute ids, so two
  prefixes bound to one namespace produced a duplicate the parser then
  rejected. Attributes are compared by expanded name.
- `document_order` and XPath node-set order were arena order, which is
  wrong after `reparent` or a recycled slot.

## [0.0.9] - 2026-10-04

### Added

- **Generational slot free-list recycling.** Removed nodes and attributes
  recycle their arena slots in `Document`, bounding memory during rapid
  tree mutation while invalidating stale `NodeId` handles via generation
  bumping.
- **Asynchronous streaming reader.** `oxml::stream::AsyncReader<R>`
  delivers non-blocking event-based streaming from any
  `tokio::io::AsyncBufRead` under optional feature `async`.
- **Diagnostic telemetry tracing.** `tracing` spans
  instrument parsing, XPath evaluation, serialization, and streaming
  under optional feature `tracing`.
- **Benchmarks and examples.** Added benchmark suites for arena
  recycling and async streaming, accompanied by runnable examples for
  async streaming and telemetry tracing.

### Performance

- **Chunked safe delimiter scanning (SWAR).** 8-byte chunk scanning
  accelerates character data and attribute value scanning without unsafe
  code, more than doubling streaming event throughput in the crate's
  own `throughput` benchmark on the recording machine.
- **Scope-aware entity memoization.** Caches validated entity
  replacement markup by entity name and active namespace scope to
  eliminate repeated throwaway tree parsing across thousands of
  references.

## [0.0.8] - 2026-08-29

### Added

- **Serialisation.** `Document::to_xml()` returns a document as XML,
  and `write_xml<W: Write>()` writes it to any writer. The parser
  could read a document and never write one back, so any round trip
  had to be done by hand.

  Escaping is done where it changes meaning rather than everywhere:
  `&`, `<` and `>` in text; additionally `"`, tab, newline and
  carriage return inside attribute values, because an unescaped
  newline there is normalised to a space on the way back in and the
  round trip would not be one. A literal carriage return becomes
  `&#13;` for the same reason.

  `<?xml version="1.1"?>` is emitted only when the document needs it
  -- a C0 or C1 control character, or a namespace prefix unbound with
  `xmlns:p=""`, neither of which XML 1.0 can express.

  `tests/serialise.rs` walks every document in the conformance corpus
  and asserts each is a fixed point: parse, serialise, parse again,
  and the two trees agree. It asserts it checked more than 1,000
  documents, so it cannot pass by finding none -- an earlier corpus
  probe in this suite reported success over zero files.

### Fixed

- **Streaming was slower than it needed to be.** `ensure_construct`
  scanned for a complete construct on every call, including when the
  whole input was already in memory and no scan could change the
  answer. Reading from a `&str` now skips it entirely. The events
  benchmark moved from 0.092x to 0.147x of the in-memory reader
  against a 0.113x baseline.

- **A generic function nothing calls is not "covered".** The
  example-coverage gate read a blank llvm-cov count as covered. A
  generic function that no example instantiates produces exactly that
  -- no caller, so no code, so no count -- which let an uncalled
  public generic pass the check that exists to catch it.

### Security

- **Every GitHub Action is pinned by commit SHA.** A mutable ref means
  the code that runs in CI can change without a commit here.
  `dtolnay/rust-toolchain@1.98.0` and
  `taiki-e/install-action@cargo-llvm-cov` were the awkward cases: the
  ref carried a *value* (which compiler, which tool) rather than a
  version, so the value moved into a `with:` input.

- **Branch coverage is measured and gated**, and Scorecard results are
  no longer uploaded to code scanning -- they are posture scores, not
  defects, and 86 of them across the suite were drowning out anything
  real.

- The Developer Certificate of Origin is adopted and enforced in CI.

## [0.0.7] - 2026-08-28

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
