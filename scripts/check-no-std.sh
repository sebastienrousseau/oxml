#!/usr/bin/env bash
#
# `no_std` is a claim about every feature combination on every target,
# not about the one configuration CI happened to build.
#
# It was one configuration until a `Vec` that resolved through the
# `std` prelude shipped and broke all three bare-metal jobs at once.
# The build that would have caught it was not being run.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

TOOLCHAIN="${OXML_TOOLCHAIN:-1.98.0}"
TARGETS=(thumbv7em-none-eabihf riscv32imac-unknown-none-elf aarch64-unknown-none)
# Every combination with `std` off. `xpath` alone is deliberately
# impossible and is checked separately below.
COMBOS=("" "libm" "xpath,libm")
FAILED=0

echo "== every no_std feature combination, on every bare-metal target =="
for target in "${TARGETS[@]}"; do
  for combo in "${COMBOS[@]}"; do
    if [ -z "$combo" ]; then
      label="(no features)"
      args=(--no-default-features)
    else
      label="$combo"
      args=(--no-default-features --features "$combo")
    fi
    if cargo "+$TOOLCHAIN" build -p oxml --target "$target" "${args[@]}" \
        > /tmp/oxml-nostd.log 2>&1; then
      printf '  %-30s %-14s ok\n' "$target" "$label"
    else
      printf '  %-30s %-14s FAIL\n' "$target" "$label"
      tail -15 /tmp/oxml-nostd.log | sed 's/^/      /'
      FAILED=1
    fi
  done
done

echo
echo "== xpath without std or libm must fail, with an explanation =="
# A link error here would be a bad failure: the fix is not discoverable
# from it. The guard in float.rs turns it into a compile error that
# names the feature to add.
out="$(cargo "+$TOOLCHAIN" build -p oxml --target "${TARGETS[0]}" \
        --no-default-features --features xpath 2>&1 || true)"
if grep -q 'requires feature `libm`' <<< "$out"; then
  echo "  refused with the intended message                    ok"
else
  echo "  FAIL: expected a compile_error naming libm"
  echo "$out" | tail -15 | sed 's/^/      /'
  FAILED=1
fi

echo
echo "== std:: must appear only behind its feature gate =="
# `#![no_std]` catches this at build time for the configurations above,
# but only for code those configurations compile. A `std::` reachable
# from a combination nobody builds would sit there unnoticed.
while IFS= read -r hit; do
  file="${hit%%:*}"
  line="${hit#*:}"; line="${line%%:*}"
  prev="$(sed -n "$((line - 1))p" "$file")"
  case "$prev" in
    *'cfg(feature = "std")'*|*'cfg(all(feature = "std"'*) ;;
    *)
      echo "  FAIL: $file:$line is not gated on the std feature"
      echo "        $prev"
      FAILED=1
      ;;
  esac
done < <(grep -rn '\bstd::' crates/oxml/src/ | grep -v '^\S*:[0-9]*://' | grep -v '///')

[ "$FAILED" -eq 0 ] && echo "  all std:: uses are gated                             ok"

echo
if [ "$FAILED" -eq 0 ]; then
  echo "no_std: fully compliant"
  exit 0
fi
echo "no_std: NOT compliant"
exit 1
