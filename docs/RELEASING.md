<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Releasing

The suite ships one version across six crates. The order is
load-bearing, and this document was **wrong about it** the first time —
the correction is below, because the mistake is easy to repeat.

## The dependency order

`oxml-cli` and `oxml-mcp` depend on **`xmlschema`**, which itself
depends on `oxml`. Publishing `xmlschema` late leaves those two
resolving *two incompatible `oxml` versions in one tree*, which fails
with `expected oxml::tree::Document, found Document` — a message that
takes a moment to recognise for what it is.

```
oxml
 └── xmlschema
      ├── oxml-cli
      └── oxml-mcp
oxml-wasm, oxml-lsp   (depend on oxml only)
```

**Publish in that order.** This file previously called `xmlschema` "a
plain bump" and listed it last.

## The API constraint

Any release that changes what an XPath expression *means* has to reach
the crates that pass expressions through. 0.0.4 resolved namespace
prefixes and made an unbound prefix an error, so three crates needed a
binding mechanism **in the same commit as the dependency bump**:

| Crate | Added in 0.0.4 |
|---|---|
| `oxml-cli` | `-n, --ns PREFIX=URI` |
| `oxml-wasm` | optional `["PREFIX=URI"]` on the query methods |
| `oxml-mcp` | `namespaces` on `xml_query`, and namespaces reported by `xml_inspect` |

Bump without the mechanism and a previously-wrong answer becomes an
error with no remedy, which is worse than either.

## Sequence

1. Merge and publish `oxml`.
2. Merge and publish `xmlschema`.
3. Per remaining satellite, in **one** commit: bump its version, bump
   its dependencies, add whatever API the release requires. Then
   publish.
4. Confirm: `curl -s https://crates.io/api/v1/crates/<name>` for each.

`main` is protected on every repository, so every step is a pull
request. That is the point — a release is not a reason to push to a
protected branch.

## Before each publish

```bash
./scripts/gate.sh          # oxml; the satellites have narrower gates
cargo publish --dry-run
```

## What went wrong in 0.0.4, so it does not go wrong again

- **A `target` symlink was committed.** A local per-project target-dir
  scheme puts a symlink at `target`, and a `git add -A` swept it into
  `xmlschema`. `.gitignore` does not save you: a path already tracked
  is not ignored. CI could not create its build directory on top of it
  — `failed to create directory .../target — Not a directory` — and
  **every build job on that repository failed** until it was removed.
  Check `git ls-files | grep '^target$'` before committing.
- **Coverage measured through a subprocess does not count.**
  `oxml-cli`'s example scripts drive the compiled binary through
  `Command`, which `llvm-cov` cannot instrument, so new code was
  covered in practice and uncovered on paper. Test the logic directly
  as well.
- **`cargo test` does not type-check `#[wasm_bindgen_test]`.** Those
  compile to nothing on a native target. `wasm-pack test --node` is
  the only thing that checks them, and it is what CI runs.
- **A test can pass on macOS and fail on Linux.** `oxml frobnicate`
  exits without reading stdin, so a harness writing to its stdin races
  the exit: Linux reports EPIPE, macOS absorbs it.
- **A behavioural change can turn an existing test into an assertion
  of a defect.** `oxml-mcp` asserted that a document containing U+0001
  is *accepted*; 0.0.4 correctly rejects it. Read what a newly-failing
  test was actually claiming before fixing it.
