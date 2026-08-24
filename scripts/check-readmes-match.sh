#!/usr/bin/env bash
# The repo-root README and the crate README must be the same file.
#
# They are two files because GitHub reads the first and docs.rs reads
# the second, and `include_str!` cannot reach outside the package. But
# only the crate one is compiled as doctests, so when they drift it is
# always the root one -- the one people actually read -- that goes
# stale. This session found it claiming "entity expansion is not
# supported" months after entity expansion was implemented, with a
# code block asserting an error that no longer occurred. The doctests
# were green the whole time, because they were running against the
# other file.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

if ! diff -u README.md crates/oxml/README.md; then
  echo
  echo "README.md and crates/oxml/README.md have diverged."
  echo "Only crates/oxml/README.md is compiled as doctests, so edit"
  echo "that one and copy it to the root:"
  echo
  echo "    cp crates/oxml/README.md README.md"
  exit 1
fi
echo "READMEs match ($(wc -l < README.md | tr -d ' ') lines)"
