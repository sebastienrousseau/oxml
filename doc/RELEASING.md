<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Releasing

The suite ships one version across seven crates. The order is
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
 ├── xmlschema
 │    ├── oxml-cli
 │    └── oxml-mcp
 ├── oxml-wasm
 └── oxml-lsp
oxml-json
 └── oxml-lsp
```

`oxml-json` is a separate root in the dependency graph and must be
published before `oxml-lsp`. It has no runtime dependency on `oxml`.

**Publish dependencies before consumers.** This file previously called
`xmlschema` "a plain bump" and listed it last.

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
3. Merge and publish `oxml-json` before `oxml-lsp`; it may be prepared
   independently of the XML library.
4. Per remaining satellite, bump its version and dependencies and add
   whatever API the release requires before merging and publishing.
5. Confirm: `curl -s https://crates.io/api/v1/crates/<name>` for each.

All seven release branches must be prepared before declaring the suite
ready. Publishing still follows dependency order; a green check against
an older published dependency does not prove compatibility with 0.0.11.
For the current assessment, see
[0.0.11 readiness](release-notes/0.0.11-readiness.md).

`main` is protected on every repository, so every step is a pull
request. That is the point — a release is not a reason to push to a
protected branch.

## Before each publish

```bash
./scripts/gate.sh          # oxml; the satellites have narrower gates
cargo publish --dry-run
```

## What the tag does

Before pushing a tag, verify that its target is the approved release
commit on `main`. Pushing `vX.Y.Z` runs `.github/workflows/release.yml`,
which refuses to continue unless:

- the tag matches `Cargo.toml`;
- the tag is an annotated tag signed by a key listed in
  `.github/allowed_signers`, with the message `oxml vX.Y.Z`;
- `doc/release-notes/X.Y.Z.md` exists and has a `## Highlights`
  section.

It then publishes through crates.io **Trusted Publishing**: the
runner exchanges a GitHub OIDC token for a short-lived crates.io
token, so no `CARGO_REGISTRY_TOKEN` is stored. Trusted Publishing is
enabled once per crate on crates.io (Settings, Trusted Publishing:
repository `sebastienrousseau/oxml`, workflow `release.yml`,
environment none). Until that is done the publish step fails to
authenticate; nothing is half-published, because the preflight runs
first and the page is composed only after the registry confirms.

Last, it creates the GitHub release page: the Highlights from the
notes file, the generated pull-request list, and a `## Checksums`
block for the `.crate`, a CycloneDX SBOM and the checksum file, each
with SLSA provenance attached (`gh attestation verify <file> --repo
sebastienrousseau/oxml`).

`scripts/publish.sh` remains for a dry run only. A real publish from
a laptop bypasses the signature check and the attestation.

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
