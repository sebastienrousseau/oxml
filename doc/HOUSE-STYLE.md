# House style for the oxml ecosystem

Every repository in the suite — `oxml`, `xmlschema`, `oxml-cli`,
`oxml-mcp`, `oxml-lsp`, `oxml-wasm` — conforms to this document.

It exists because "match noyalib" is not a checkable instruction. This
is: each item below is either present or it is not, and CI can say
which.

The reference is
[noyalib](https://github.com/sebastienrousseau/noyalib), measured on
2026-08-23: a 1,633-line README, 50 files under `doc/`, 82 examples,
16 benchmarks.

## Measured starting point

| Repo | README | doc/ | examples | benches |
|---|---|---|---|---|
| **noyalib** (reference) | **1,633** | **50** | **82** | **16** |
| oxml | 515 | 0 | 1 | 2 |
| xmlschema | 224 | 0 | 1 | 0 |
| oxml-cli | 107 | 0 | 0 | 0 |
| oxml-mcp | 101 | 0 | 0 | 0 |
| oxml-lsp | 93 | 0 | 0 | 0 |
| oxml-wasm | 104 | 0 | 0 | 0 |

## README sections, in order

Taken from noyalib's own headings. A repository omits a section only
when it does not apply — a library has no "Install the binaries" — and
never silently.

1. Title, one-line description, badge block
2. `## Contents`
3. `## Install` — library, CLI, `no_std`, from source, cargo features
4. `## Quick Start`
5. `## The oxml ecosystem` — what each crate is, and per-host links
6. `## Migration from …` — one per credible alternative
7. `## Why this approach?`
8. `## Capabilities in <version>` — a release inventory
9. `## Ecosystem comparison` — a table against real competitors
10. `## Benchmarks` — numbers, with the machine and method stated
11. `## Features` — cargo features, what each costs
12. Feature-by-feature sections, one per capability
13. `## Library Usage`
14. `## Configuration`
15. `## Examples` — pointing at `examples/`, all of which compile in CI
16. `## When not to use <crate>`
17. `## FAQ`
18. `## Contributing`, `## Licence`, `## Acknowledgements`

## `doc/` layout

```
doc/
├── ARCHITECTURE.md          how it works, and why it is shaped this way
├── BENCHMARKS.md            method, machine, numbers, how to reproduce
├── COMPARISON.md            against libxml2, quick-xml, roxmltree, …
├── CONFORMANCE.md           W3C suite results (oxml, xmlschema)
├── ECOSYSTEM.md             the six crates and how they fit
├── MIGRATION-FROM-*.md      one per alternative crate
├── MSRV-AND-DEPRECATION.md  the support policy
├── SECURITY-MODEL.md        threat model; what is refused and why
├── TESTING.md               conformance, fuzzing, Miri, properties
├── USER-GUIDE.md            the long-form guide
├── adr/                     architecture decision records
├── design/                  design notes
├── diagrams/                mermaid sources
└── release-notes/           per-version notes
```

`doc/` holds what belongs in version control and does **not** duplicate
rustdoc. Rustdoc documents the API; `doc/` documents the decisions.

## Examples

- One per public capability, named after it.
- Every example compiles and runs in CI — the `examples` job already
  does this for `oxml`.
- An example is the second place a reader looks after the README, so it
  carries prose explaining *why*, not only *what*.

## Benchmarks

- One per capability, not one per crate.
- `BENCHMARKS.md` states the machine, the toolchain and the method.
- No figure is published from a loaded machine. This session recorded
  the same binary at 14.7 and 123.1 MB/s with a load average above 30;
  a number without its conditions is not a measurement.

## Coverage

≥95% of lines, gated in CI. **Already met**: `oxml` 97.0%, `xmlschema`
95.5%, `oxml-cli` 98.5%, `oxml-mcp` 99.2%, `oxml-lsp` 97.8%,
`oxml-wasm` 100%.

## Rustdoc

- `#![deny(missing_docs)]`, and `cargo doc` runs with
  `RUSTDOCFLAGS=-D warnings` in CI.
- Every public item documented, with `# Errors` on anything returning
  `Result` and `# Panics` where a panic is reachable.
- The crate-level doc is `#[doc = include_str!("../README.md")]`, so
  the README's examples are compiled as doctests and cannot drift.

## Tone

Match noyalib: direct, technical, and willing to say what a thing is
*not* good at. "When not to use" is a required section, not a
courtesy. Claims carry their evidence — a benchmark cites its machine,
a conformance number cites its suite release and its denominator.
