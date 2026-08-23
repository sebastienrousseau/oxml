# oxml — handover

State as of the end of the 2026-08-23 session. Everything described is
committed and pushed; nothing is half-written.

## Repositories and branches

| Repo | Branch | State |
|---|---|---|
| oxml | `feat/v0.0.4` | **PR #1**, open. Phases 0+1. CI partly red — see below. |
| oxml | `feat/phase2-borrowing` | Phase 2 perf, green locally |
| oxml | `feat/phase2-borrow-text` | Phase 2 perf, on top of the above |
| oxml-cli | `fix/interned-names` | API fix for `NameId` |
| oxml-mcp / -lsp / -wasm | `main` | toolchain pin only |
| xmlschema | PR #5 | toolchain pin (`master` is protected) |

All six crates are published at **0.0.3**. Nothing above is released.

## FIRST: finish CI on PR #1

Three job classes are still failing and were **not** diagnosed. Read the
logs; do not infer.

```sh
cd /tmp/oxml && gh pr checks 1
rid=$(gh run list --branch feat/v0.0.4 --limit 1 --json databaseId -q '.[0].databaseId')
gh run view "$rid" --log-failed | grep -E '^Fuzz|^Coverage|^cargo deny'
```

- **Fuzz (×5)** — `RUSTUP_TOOLCHAIN: nightly` fixed Miri the same way, so
  the remaining cause is probably the cargo-fuzz install step or the
  seed-copy step, but that is a guess.
- **Coverage** — likely the same toolchain interaction; `cargo llvm-cov`
  needs `llvm-tools-preview` on the toolchain that actually runs.
- **cargo deny** — lives in `security.yml`, not `ci.yml`.

Already fixed and awaiting a re-run: **MSRV**, **Benchmarks**.
Already passing: Lint, Miri, all three `no_std`, tests on 3 platforms,
W3C conformance, feature powerset, examples, forbid-unsafe, audit.

## The single most important lesson

**Do not trust a local green.** This session reported "all green"
repeatedly while CI was red. Causes found, all of the same shape — a
check that appears to run and does not:

- `RUSTUP_TOOLCHAIN=1.97.1` is exported in the developer environment and
  overrides `rust-toolchain.toml`. CI ran 1.98.0. A clippy lint added in
  1.98 failed CI while passing locally.
- `cargo test` never runs benches, so a benchmark broken since Phase 0
  went unnoticed.
- The MSRV job installed 1.86.0 and then built on 1.98.0.
- An unseeded fuzz target completed 100,000 runs testing almost nothing.
- The conformance loader silently dropped all 159 Sun tests.
- A float equivalence test compared `std` against `std`.

All six were caught by reading the *number*, not the status.
`rust-toolchain.toml` now pins `1.98.0` exactly in all six repos.

## Achievements this session

**Conformance** (`oxml`): 47.9% → **93.6%** pass, 70.8% → **98.9%**
coverage, 0 panics, against `xmlts20130923` (2,585 tests), with a
baseline ratchet that fails on regression *and* on unreviewed
improvement.

**Phase 0**: no_std XPath via `libm`, `Limits` (10 bounds), 5 fuzz
targets, Miri, 9 property tests.

**Phase 2 perf**: allocations **3.45 → 2.00 per node** (3.8M → 2.2M),
2× measured throughput from arena pre-sizing.

**Bugs found and fixed**: 3 XPath 1.0 conformance defects, an unbounded
parser recursion (process abort), a quadratic entity blowup (100 MB from
100 KB), and a comment-before-DOCTYPE parse failure.

## Next work, in order

1. **Green CI, then merge PR #1.** 93.6% conformance with a ratchet is a
   claim no other Rust XML crate makes, and it is finished and unmerged.
2. **Finish Phase 2**: ~800k allocations remain in text nodes and
   attribute values. Use the **owned-input** design in
   `doc/HOUSE-STYLE.md`'s sibling notes — `Document` owns a `String` and
   nodes hold `(start, len)` ranges — *not* a lifetime parameter. It
   gets the same win with no lifetime propagation and no awkward
   `parse_bytes` variant, at the cost of one memcpy (~0.44 ms against
   ~48 ms of allocation).
3. **Measure throughput on an idle machine.** Load average hit 34.75
   here and one binary read 14.7–123.1 MB/s. Publish nothing until quiet.
4. **Streaming/index API** as a third entry point — a different tool,
   not a faster tree. `oxml-lsp` and `xmlschema` are single-pass and
   would use it.
5. **Documentation to house style** — `doc/HOUSE-STYLE.md` on
   `feat/v0.0.4` is the checklist. `oxml` has its FAQ; five more to go,
   plus `doc/` folders and examples. Do one repo completely before
   starting the next.

## Gotchas that will cost an hour each

- **Satellites need `[patch.crates-io]` for BOTH `oxml` and
  `xmlschema`** when testing against an unreleased core. Patching one
  gives `expected oxml::tree::Document, found Document`.
- **`xmlschema`'s default branch is `master`**, protected, `enforce_admins`
  on. It needs a PR.
- **The W3C suite download needs a browser User-Agent.** Without one,
  w3.org returns a 5,850-byte Cloudflare challenge page with HTTP 200,
  and the conformance job passes having run zero tests. The downloader
  verifies a SHA-256 for this reason.
- **`NodeKind::Element` carries `NameId` and `(u32,u32)`** on the phase2
  branches — a breaking change. Use `Document::element_name()`.
- **Per-project target dirs**: `/tmp/builds/<name>` via symlink, plus
  sccache. A shared `target-dir` serialises every repo behind one lock.
