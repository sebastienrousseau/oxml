#!/usr/bin/env bash
#
# Everything CI runs, locally, in the order that fails fastest.
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
cd "$(git rev-parse --show-toplevel)"

TOOLCHAIN="${OXML_TOOLCHAIN:-1.98.0}"
MSRV="${OXML_MSRV:-1.86.0}"
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
step "no tracked build dir" bash -c '! git ls-files | grep -qx target'
step "fmt"            cargo "+$TOOLCHAIN" fmt --all --check
step "clippy"         cargo "+$TOOLCHAIN" clippy --workspace --all-targets --all-features -- -D warnings
step "tests"          cargo "+$TOOLCHAIN" test --workspace --all-features
step "no_std"         cargo "+$TOOLCHAIN" build -p oxml --no-default-features --features libm,xpath
step "no_std minimal" cargo "+$TOOLCHAIN" build -p oxml --no-default-features
step "feature powerset" \
  cargo "+$TOOLCHAIN" hack check -p oxml --feature-powerset --no-dev-deps --group-features xpath,libm

step "no_std audit"   ./scripts/check-no-std.sh
step "conformance"    cargo "+$TOOLCHAIN" test -p oxml-conformance --release
step "rustdoc"        env RUSTDOCFLAGS="-D warnings" cargo "+$TOOLCHAIN" doc --no-deps -p oxml --all-features
step "READMEs match"  ./scripts/check-readmes-match.sh
step "doc tests generated" bash -c '
  python3 scripts/generate-doc-tests.py >/dev/null &&
  git diff --exit-code crates/oxml/tests/doc_examples.rs'
step "examples cover the API" python3 scripts/check-example-coverage.py

# Examples are run by CI one at a time; a failure in any is a failure.
step "examples run" bash -c '
  for f in crates/oxml/examples/*.rs; do
    cargo test --quiet --no-run >/dev/null 2>&1
    cargo run --quiet --example "$(basename "$f" .rs)" >/dev/null || exit 1
  done'

step "MSRV $MSRV" env RUSTUP_TOOLCHAIN="$MSRV" cargo build --workspace --all-features

echo
if [ "${#FAILED[@]}" -eq 0 ]; then
  echo "all green"
  exit 0
fi
printf 'FAILED: %s\n' "${FAILED[@]}"
exit 1
