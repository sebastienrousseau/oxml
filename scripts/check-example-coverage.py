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


def cargo() -> str:
    """Where cargo is, saying so plainly when it is nowhere.

    A bare `cargo` is right on CI and in a normal shell, but a rustup
    install puts the real binary in ~/.cargo/bin and a PATH that lost
    that entry produces a FileNotFoundError traceback rather than a
    sentence.
    """
    import os
    import shutil

    found = shutil.which(os.environ.get("CARGO", "cargo"))
    if found:
        return found
    fallback = pathlib.Path.home() / ".cargo" / "bin" / "cargo"
    if fallback.is_file():
        return str(fallback)
    sys.exit("cargo is not on PATH and ~/.cargo/bin/cargo does not exist")


def run(*args: str) -> str:
    args = (cargo(),) + args[1:] if args and args[0] == "cargo" else args
    proc = subprocess.run(
        args, cwd=ROOT, capture_output=True, text=True, check=False
    )
    if proc.returncode != 0:
        # Every command here must succeed. When a failing command was
        # ignored, an example that did not compile produced no coverage
        # data, the file it exercised was skipped for having none, and
        # the check reported success over an API nothing had run.
        sys.exit(
            f"command failed: {' '.join(args)}\n"
            f"{proc.stdout[-2000:]}\n{proc.stderr[-2000:]}"
        )
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
    unexercised: list[str] = []
    for path in sorted(SRC.rglob("*.rs")):
        rel = path.relative_to(ROOT / "crates" / "oxml")
        lines = path.read_text().splitlines()
        declared = [
            (n, t.strip())
            for n, t in enumerate(lines, 1)
            if t.strip().startswith(("pub fn ", "pub const fn "))
        ]
        keys = [k for k in counts if k.endswith(str(rel))]
        if not keys:
            # Not "nothing to check" -- a file with public functions and
            # no coverage data at all means the instrumented build never
            # reached it, which is a failure to measure, not a pass.
            if declared:
                unexercised.append(
                    f"{rel} has {len(declared)} public functions and no "
                    f"coverage data at all"
                )
                total += len(declared)
            continue
        for number, stripped in declared:
            total += 1
            count = counts[keys[0]].get(number)
            if count is None:
                unexercised.append(f"{rel}:{number} (no coverage data) {stripped}")
            elif count == "":
                # A blank count means llvm-cov generated no region for
                # the line. For a *generic* function that is what an
                # uninstantiated one looks like: no caller, so no code,
                # so nothing to count.
                #
                # Testing only for "0" read blank as covered, which let
                # a generic public function pass this check while no
                # example called it -- the failure this check exists to
                # prevent, inside the check. Found when the script was
                # ported to `oxml-lsp`, whose `serve` is generic.
                unexercised.append(
                    f"{rel}:{number} (never instantiated) "
                    f"{stripped.rstrip(' {')}"
                )
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
