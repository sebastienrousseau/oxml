# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Every member of the oxml suite ships the **same version number**. If the
core is at `0.0.X` then so is every satellite, so there is never a
compatibility table to consult. Versions advance in `0.0.1` steps along
the `0.0.x` line; `0.1.0` follows `0.0.999`.

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
