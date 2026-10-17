#!/usr/bin/env bash
#
# Core CI checks, locally, in the order that fails fastest. Coverage,
# Miri, fuzzing and advisory scans run separately in isolated CI jobs.
#
# This exists because three separate pushes went out red on checks that
# were green locally -- and each time the reason was the same: the
# local gate I was running by hand was a subset of CI. `no_std` and the
# feature powerset were the ones that caught me, because a `Vec` that
# resolves through the `std` prelude compiles fine until it does not.
#
# Run this before pushing. It is slower than `cargo test`; it is much
# faster than a round-trip through CI.
set -uo pipefail
# Resolve the repository from this script's own location, not from
# the caller's. `git rev-parse` reads the *current* directory, so
# running this by absolute path from anywhere else failed with
# "fatal: not a git repository".
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)" || exit 1

TOOLCHAIN="${OXML_TOOLCHAIN:-1.98.0}"
# Read from Cargo.toml, as the CI MSRV job does. A literal here went
# stale when the floor moved to 1.89.0, and the gate went on proving
# a toolchain the crate no longer promises.
MSRV="${OXML_MSRV:-$(grep -m1 '^rust-version' Cargo.toml | grep -oE '[0-9]+\.[0-9]+(\.[0-9]+)?')}"
# CI sets this for every job. This script did not, so it was weaker
# than the thing it exists to predict: a `no_std` build that only
# warned passed here and turned all three bare-metal jobs red on the
# pull request. A subset gate is the failure mode this file was
# written for; leaving one flag out reintroduced it.
export RUSTFLAGS="${RUSTFLAGS:--D warnings}"
FAILED=()

step() {
  local name="$1"; shift
  printf '%-34s' "$name"
  if "$@" > /tmp/oxml-gate.log 2>&1; then
    echo "ok"
  else
    echo "FAIL"
    FAILED+=("$name")
    tail -25 /tmp/oxml-gate.log | sed 's/^/    /'
  fi
}

# A local per-project target-dir scheme puts a symlink at `target`, and
# a `git add -A` will commit it. `.gitignore` does not help once a path
# is tracked, and CI then cannot create its build directory on top of
# it -- every build job fails until it is removed.
step "fuzz license policy" python3 scripts/test-fuzz-license-policy.py
step "example coverage contract" python3 scripts/test-example-coverage.py
step "no tracked build dir" bash -c '! git ls-files | grep -qx target'
step "fmt"            cargo "+$TOOLCHAIN" fmt --all --check
step "fmt fuzz"       cargo "+$TOOLCHAIN" fmt --manifest-path fuzz/Cargo.toml --check
step "release inputs"  python3 scripts/test-release-highlights.py
step "publish dry-run contract" python3 scripts/test-publish-dry-run.py
step "clippy"         cargo "+$TOOLCHAIN" clippy --workspace --all-targets --all-features -- -D warnings
step "tests"          cargo "+$TOOLCHAIN" test --workspace --all-features
step "tests minimal"  cargo "+$TOOLCHAIN" test -p oxml --no-default-features --locked
step "tests no_std XPath" cargo "+$TOOLCHAIN" test -p oxml --no-default-features --features xpath,libm --locked
step "no_std"         cargo "+$TOOLCHAIN" build -p oxml --no-default-features --features libm,xpath
step "no_std minimal" cargo "+$TOOLCHAIN" build -p oxml --no-default-features
step "feature powerset" \
  cargo "+$TOOLCHAIN" hack check -p oxml --feature-powerset --no-dev-deps --group-features xpath,libm

step "no_std audit"   ./scripts/check-no-std.sh
# `OXML_REQUIRE_SUITE=1` turns a missing suite from a skip into a
# failure. Without it this step printed `ok` on a fresh clone having
# run zero of the 2,585 tests, because a skipped test is a passing
# test as far as `cargo test` is concerned.
step "conformance"    env OXML_REQUIRE_SUITE=1 \
  cargo "+$TOOLCHAIN" test -p oxml-conformance --release
step "rustdoc"        env RUSTDOCFLAGS="-D warnings" cargo "+$TOOLCHAIN" doc --no-deps -p oxml --all-features
step "READMEs match"  ./scripts/check-readmes-match.sh
step "doc links"      python3 ./scripts/check-doc-links.py
step "doc tests generated" bash -c '
  python3 scripts/generate-doc-tests.py >/dev/null &&
  git diff --exit-code crates/oxml/tests/doc_examples.rs'
step "examples cover the API" python3 scripts/check-example-coverage.py

# Examples are run by CI one at a time; a failure in any is a failure.
# Each with the features its `required-features` names, read from
# Cargo metadata as the CI job reads them: a bare `cargo run
# --example` refuses `stream_async`, which needs `async`, and this
# step failed on every run once that example existed.
step "examples run" bash -s <<'EXAMPLES'
  set -euo pipefail
  meta="$(cargo metadata --no-deps --format-version 1 --locked)"
  [ -n "$meta" ]
  for f in crates/oxml/examples/*.rs; do
    name=$(basename "$f" .rs)
    feats="$(jq -r --arg n "$name" '.packages[]
      | select(.name == "oxml") | .targets[]
      | select(.kind == ["example"] and .name == $n)
      | (."required-features" // []) | join(",")' <<< "$meta")"
    cargo run --quiet --locked -p oxml --example "$name" --features "$feats" >/dev/null
  done
EXAMPLES

# What `cargo publish` would upload, and whether the assurance case in
# it describes this crate. oxml-json 0.0.8 shipped one inherited from
# oxml-lsp, claiming tests over an `analyse()` it does not have; the
# correction landed twelve minutes after the upload, and crates.io
# versions cannot be edited afterwards.
# Documented test counts against the suite that produces them. These
# decayed everywhere at once: oxml stated three different totals in
# three places, and oxml-mcp claimed 57 tests while having 40 because
# the JSON tests left with the code. A count higher than reality is a
# claim of coverage that does not exist.
step "published figures" python3 scripts/check-figures.py

step "package claims" python3 scripts/check-package.py

# Per-function complexity against the committed baseline: a new
# offender or a worse one fails; see the script for the ceilings.
step "complexity ratchet" python3 scripts/check-complexity.py

step "MSRV $MSRV" env RUSTUP_TOOLCHAIN="$MSRV" cargo build --workspace --all-features

echo
if [ "${#FAILED[@]}" -eq 0 ]; then
  echo "all green"
  exit 0
fi
printf 'FAILED: %s\n' "${FAILED[@]}"
exit 1
