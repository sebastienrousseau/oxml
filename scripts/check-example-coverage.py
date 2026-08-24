#!/usr/bin/env python3
"""Fail if any public function is not executed by at least one example.

The README claims the examples cover the public API. That claim decays
silently: a function added in one commit and documented in none is
indistinguishable, from the outside, from one the examples exercise.
This measures it instead, by running the examples under coverage
instrumentation and checking the execution count at the line each
`pub fn` is declared on.

It deliberately checks *execution*, not mention. An example that names
a function in a comment does not count.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "crates" / "oxml" / "src"
MANIFEST = ROOT / "crates" / "oxml" / "Cargo.toml"


def examples() -> list[str]:
    text = MANIFEST.read_text()
    return re.findall(r'^\[\[example\]\]\nname = "([^"]+)"', text, re.M)


def run(*args: str) -> str:
    proc = subprocess.run(
        args, cwd=ROOT, capture_output=True, text=True, check=False
    )
    if proc.returncode != 0 and "llvm-cov report" in " ".join(args):
        sys.exit(f"coverage report failed:\n{proc.stderr}")
    return proc.stdout


def main() -> int:
    names = examples()
    if not names:
        sys.exit("no examples declared in Cargo.toml")
    print(f"running {len(names)} examples under instrumentation")

    run("cargo", "llvm-cov", "clean", "--workspace")
    for name in names:
        run(
            "cargo", "llvm-cov", "--no-report", "run",
            "-q", "-p", "oxml", "--example", name,
        )
    report = run("cargo", "llvm-cov", "report", "--text")

    # file -> {line number: execution count}
    counts: dict[str, dict[int, str]] = {}
    current = None
    for line in report.splitlines():
        header = re.match(r"^(/.*\.rs):$", line)
        if header:
            current = header.group(1)
            counts[current] = {}
            continue
        if current:
            row = re.match(r"^\s*(\d+)\|\s*([\d.kMe]*)\|", line)
            if row:
                counts[current][int(row.group(1))] = row.group(2).strip()

    total = 0
    unexercised = []
    for path in sorted(SRC.rglob("*.rs")):
        rel = path.relative_to(ROOT / "crates" / "oxml")
        keys = [k for k in counts if k.endswith(str(rel))]
        if not keys:
            continue
        for number, text in enumerate(path.read_text().splitlines(), 1):
            stripped = text.strip()
            if not stripped.startswith(("pub fn ", "pub const fn ")):
                continue
            total += 1
            count = counts[keys[0]].get(number)
            if count is None:
                unexercised.append(f"{rel}:{number} (no coverage data) {stripped}")
            elif count == "0":
                unexercised.append(f"{rel}:{number} {stripped.rstrip(' {')}")

    if total == 0:
        sys.exit("found no `pub fn` declarations -- the check is not working")

    print(f"{total - len(unexercised)}/{total} public functions exercised")
    if unexercised:
        print("\nnot reached by any example:")
        for item in unexercised:
            print(f"  {item}")
        print("\nAdd an example that calls it, or make it private.")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
