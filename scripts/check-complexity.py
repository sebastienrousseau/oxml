#!/usr/bin/env python3
"""Fail if a function is more complex than the committed baseline allows.

Every function in `crates/oxml/src` is measured with
`rust-code-analysis-cli` against four ceilings: cyclomatic 10,
cognitive 15, Halstead difficulty 30, 60 source lines. A function over
any ceiling is an offender. `complexity-baseline.tsv` lists the
offenders that already exist, with their numbers.

The rules are a ratchet:

  * an offender missing from the baseline fails (new or renamed code
    must come in under the ceilings);
  * an offender that got worse than its baseline row fails;
  * an offender that improved, or disappeared, passes and prints a note
    to run `--update`, which rewrites the baseline so the backlog can
    only shrink.

`--update` never adds a row and never raises a number; it only records
improvements. A regression is fixed in the code, not absorbed here.
"""

from __future__ import annotations

import json
import pathlib
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SOURCES = ROOT / "crates" / "oxml" / "src"
BASELINE = ROOT / "complexity-baseline.tsv"
METRICS = ("cyclomatic", "cognitive", "halstead", "sloc")
CEILINGS = {"cyclomatic": 10, "cognitive": 15, "halstead": 30, "sloc": 60}
HEADER = "file\tfunction\t" + "\t".join(METRICS)

Row = dict[str, int]


def tool() -> str:
    found = shutil.which("rust-code-analysis-cli")
    if not found:
        sys.exit(
            "rust-code-analysis-cli is not on PATH; install it with\n"
            "    cargo install rust-code-analysis-cli --locked"
        )
    return found


def measure_file(path: pathlib.Path) -> dict:
    proc = subprocess.run(
        [tool(), "-m", "-O", "json", "-p", str(path)],
        capture_output=True, text=True, check=True,
    )
    return json.loads(proc.stdout)


def metrics_of(space: dict) -> Row:
    m = space["metrics"]
    return {
        "cyclomatic": round(m["cyclomatic"]["sum"]),
        "cognitive": round(m["cognitive"]["sum"]),
        "halstead": round(m["halstead"].get("difficulty") or 0),
        "sloc": round(m["loc"]["sloc"]),
    }


def walk(space: dict, file: str, scope: list[str], out: dict[str, Row]) -> None:
    """Collect every function, keyed by file and enclosing scope."""
    name = space.get("name") or ""
    inner = scope + [name] if space.get("kind") in ("impl", "trait", "mod") else scope
    if space.get("kind") == "function":
        key = file + "\t" + "::".join(inner + [name])
        # Two functions with the same scope and name (cfg-gated twins)
        # share a row; keep the worse of the two.
        row = metrics_of(space)
        prior = out.get(key)
        out[key] = row if prior is None else {k: max(prior[k], row[k]) for k in METRICS}
    for child in space.get("spaces", []):
        walk(child, file, inner, out)


def measure_all() -> dict[str, Row]:
    found: dict[str, Row] = {}
    for path in sorted(SOURCES.rglob("*.rs")):
        rel = path.relative_to(ROOT).as_posix()
        walk(measure_file(path), rel, [], found)
    return found


def offenders(rows: dict[str, Row]) -> dict[str, Row]:
    return {k: r for k, r in rows.items() if any(r[m] > CEILINGS[m] for m in METRICS)}


def read_baseline() -> dict[str, Row]:
    if not BASELINE.is_file():
        return {}
    lines = BASELINE.read_text().splitlines()
    rows: dict[str, Row] = {}
    for line in lines[1:]:
        if not line.strip():
            continue
        file, function, *values = line.split("\t")
        rows[file + "\t" + function] = dict(zip(METRICS, map(int, values)))
    return rows


def write_baseline(rows: dict[str, Row]) -> None:
    body = [HEADER]
    for key in sorted(rows):
        body.append(key + "\t" + "\t".join(str(rows[key][m]) for m in METRICS))
    BASELINE.write_text("\n".join(body) + "\n")


def worse(now: Row, then: Row) -> list[str]:
    return [m for m in METRICS if now[m] > then[m] and now[m] > CEILINGS[m]]


def describe(key: str, row: Row) -> str:
    file, function = key.split("\t")
    numbers = " ".join(f"{m}={row[m]}" for m in METRICS)
    return f"{file} {function}: {numbers}"


def compare(now: dict[str, Row], then: dict[str, Row]) -> tuple[list[str], list[str]]:
    """(failures, improvements) between the current offenders and the baseline."""
    failures: list[str] = []
    improvements: list[str] = []
    for key, row in sorted(now.items()):
        prior = then.get(key)
        if prior is None:
            failures.append("new offender: " + describe(key, row))
        elif worse(row, prior):
            failures.append(f"regressed ({', '.join(worse(row, prior))}): " + describe(key, row))
        elif any(row[m] < prior[m] for m in METRICS):
            improvements.append(describe(key, row))
    improvements += [describe(k, r) + " (gone)" for k, r in then.items() if k not in now]
    return failures, improvements


def update(now: dict[str, Row], then: dict[str, Row]) -> None:
    """Record improvements only: no new rows, no higher numbers."""
    kept = {k: {m: min(r[m], then[k][m]) for m in METRICS} for k, r in now.items() if k in then}
    write_baseline(kept)
    print(f"baseline rewritten: {len(kept)} offenders (was {len(then)})")


def main(argv: list[str]) -> int:
    now = offenders(measure_all())
    then = read_baseline()
    if "--update" in argv:
        update(now, then)
        return 0
    failures, improvements = compare(now, then)
    for line in improvements:
        print("improved: " + line)
    if improvements:
        print("run `python3 scripts/check-complexity.py --update` to record this")
    for line in failures:
        print("::error::" + line)
    if failures:
        print(f"\n{len(failures)} function(s) over the ceilings and not covered by the baseline")
        return 1
    print(f"complexity ok: {len(now)} baselined offenders, none worse")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
