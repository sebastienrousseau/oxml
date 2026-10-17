#!/usr/bin/env bash
#
# Validate this crate's release package from a laptop, without uploading.
#
# CI publishes on a signed tag through crates.io Trusted Publishing
# (.github/workflows/release.yml). A laptop upload bypasses that path's
# signature verification and artifact attestations, so this helper only
# checks the reviewed main tree and runs Cargo's release dry run.
#
# crates.io versions are immutable; failures must stop before publication.
set -euo pipefail
# Resolve the repository from this script's own location, not from
# the caller's. `git rev-parse` reads the *current* directory, so
# running this by absolute path from anywhere else failed with
# "fatal: not a git repository".
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

CRATE="oxml"
PKG_ARG=(-p oxml)

version="$(cargo metadata --no-deps --format-version 1 --locked \
  | jq -r --arg n "$CRATE" '.packages[] | select(.name==$n) | .version')"
echo "== $CRATE $version =="

# 1. Releasing anything other than main's tip publishes a tree nobody
#    reviewed.
branch="$(git rev-parse --abbrev-ref HEAD)"
[ "$branch" = "main" ] || { echo "on $branch, not main"; exit 1; }
git fetch origin --quiet
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] \
  || { echo "main is not level with origin/main"; exit 1; }
[ -z "$(git status --porcelain)" ] \
  || { echo "working tree is dirty; cargo publish would package it"; exit 1; }

# 2. The core has no sibling dependencies to wait for. Run the local
#    gate; CI additionally checks coverage, Miri, fuzzing and advisories.
./scripts/gate.sh

# 3. What the upload would contain, using the reviewed dependency lock.
cargo publish --dry-run "${PKG_ARG[@]}" --locked

echo "Dry run complete for $CRATE $version. Nothing was published."
echo "Publication requires the reviewed signed tag and release workflow."
