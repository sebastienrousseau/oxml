# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Every member of the oxml suite ships the **same version number**. If the
core is at `0.0.X` then so is every satellite, so there is never a
compatibility table to consult. Versions advance in `0.0.1` steps along
the `0.0.x` line; `0.1.0` follows `0.0.999`.

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
