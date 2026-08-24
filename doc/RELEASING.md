<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Releasing 0.0.4

The suite ships one version across six crates. This release contains a
change that makes the **order** load-bearing, so the sequence below is
not a formality.

## The hazard

oxml 0.0.4 resolves namespace prefixes in XPath name tests, and an
unbound prefix is now a compile error rather than a silent match on the
local part. Three satellites accept expressions and pass them straight
through, and **none can supply bindings**:

| Crate | Needs, before its dependency is bumped |
|---|---|
| `oxml-cli` | `-n, --ns PREFIX=URI` — specified in [oxml-cli `doc/NAMESPACES.md`](https://github.com/sebastienrousseau/oxml-cli/blob/main/doc/NAMESPACES.md) |
| `oxml-wasm` | a second argument on `queryText` / `queryValue` / `queryCount` |
| `oxml-mcp` | an optional `namespaces` argument on `xml_query`, and namespaces reported by `xml_inspect` |

Bump a satellite's dependency without adding its binding mechanism and
namespaced queries become **impossible** in that tool: a
previously-wrong answer turns into an error with no remedy, which is
worse than either.

So for each satellite, **the dependency bump and the binding API are
one change**, not two commits.

## Sequence

1. **Merge and publish `oxml` 0.0.4.** Its own version is already
   bumped; nothing depends on anything unpublished.
2. **Per satellite, in one commit each:** bump `oxml = "0.0.4"`, bump
   the crate's own version to `0.0.4`, and add its binding API.
3. **Publish the satellites.**
4. `xmlschema` has no expression surface, so step 2 is a plain bump for
   it.

Until step 1 lands, every satellite keeps `oxml = "0.0.3"` and its own
version at `0.0.3`. A satellite at 0.0.4 depending on oxml 0.0.3 would
break the one-version contract in a way no compiler catches.

## What `oxml-cli`'s example already knows

`examples/query-basics.sh` skips one assertion behind
`OXML_NAMESPACE_FIX`, with a printed reason rather than a comment,
because selecting an attribute by namespace needs 0.0.4. When the
dependency is bumped, that guard comes out and the assertion runs.

## Before publishing

```bash
./scripts/gate.sh          # 14 steps, matches CI
cargo publish --dry-run -p oxml
```

The gate covers `no_std` across nine build configurations, the W3C
conformance ratchet, Miri, MSRV 1.86.0, and the checks that keep the
documentation honest — README parity, generated doc tests, and every
public function being reachable from an example.
