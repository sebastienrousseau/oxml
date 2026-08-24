#!/usr/bin/env python3
"""Every markdown link must resolve.

Four anchors were broken when this was written, all of them contents
entries pointing at headings that had since been renamed -- including
one titled "Entity expansion is not supported" for a section that says
the opposite. A link that 404s is a small thing; a contents entry that
misstates what a section concludes is not.

Checks, across every tracked `.md` file:

  * in-page anchors resolve to a heading in the same file
  * relative file links point at something that exists
  * cross-file anchors resolve to a heading in the target file
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SKIP = {".git", "target", "node_modules", "pkg"}

# GitHub's anchor algorithm: lowercase, drop anything but word
# characters, spaces and hyphens, then spaces to hyphens.
def anchor(heading: str) -> str:
    a = heading.strip().lower()
    a = a.replace("`", "")
    a = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", a)  # links keep their text
    a = re.sub(r"<[^>]+>", "", a)                    # inline html
    a = re.sub(r"[^\w\s-]", "", a)
    return re.sub(r"\s+", "-", a.strip())


def headings(text: str) -> set[str]:
    return {anchor(m.group(2)) for m in re.finditer(r"^(#{1,6})\s+(.+)$", text, re.M)}


def main() -> int:
    files = [p for p in ROOT.rglob("*.md") if not SKIP & set(p.parts)]
    cache = {p: p.read_text(encoding="utf-8") for p in files}
    heads = {p: headings(t) for p, t in cache.items()}

    problems: list[str] = []
    for path, text in cache.items():
        rel = path.relative_to(ROOT)
        for m in re.finditer(r"\[[^\]]*\]\(([^)\s]+)\)", text):
            target = m.group(1)
            if target.startswith(("http://", "https://", "mailto:")):
                continue
            file_part, _, frag = target.partition("#")
            if not file_part:
                if frag not in heads[path]:
                    problems.append(f"{rel}: no heading for #{frag}")
                continue
            dest = (path.parent / file_part).resolve()
            if not dest.exists():
                problems.append(f"{rel}: {file_part} does not exist")
            elif frag and dest.suffix == ".md" and dest in heads:
                if frag not in heads[dest]:
                    problems.append(f"{rel}: {file_part} has no heading #{frag}")

    if problems:
        print(f"{len(problems)} broken markdown link(s):")
        for p in problems:
            print(f"  {p}")
        return 1
    print(f"all markdown links resolve ({len(files)} files)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
