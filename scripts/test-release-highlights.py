#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0 OR MIT
"""Exercise release-note and package-metadata checks without publishing."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


def extraction_command() -> str:
    workflow = Path(__file__).resolve().parent.parent / ".github/workflows/release.yml"
    for line in workflow.read_text().splitlines():
        command = line.strip()
        if command.startswith(("sed -n ", "awk ")) and '"$notes"' in command:
            return command
    raise AssertionError("release workflow has no highlights extraction command")


def vcs_check_command() -> str:
    workflow = Path(__file__).resolve().parent.parent / ".github/workflows/release.yml"
    for line in workflow.read_text().splitlines():
        command = line.strip().removeprefix("| ")
        if command.startswith('jq -e --arg sha "$GITHUB_SHA"'):
            return command
    raise AssertionError("release workflow has no package source-commit check")


class ReleaseHighlights(unittest.TestCase):
    def test_heading_and_last_bullet_survive_once(self) -> None:
        expected = "## Highlights ⭐️\n\n* **First**: retained.\n* **Last**: retained."
        for suffix in ["", "\n", "\n\n## Upgrade notes\nNot a highlight.\n"]:
            with self.subTest(suffix=suffix), tempfile.TemporaryDirectory() as directory:
                notes = Path(directory) / "notes.md"
                notes.write_text("# Release\n\n" + expected + suffix)
                result = subprocess.run(
                    ["bash", "-o", "pipefail", "-c", extraction_command()],
                    env={"PATH": os.defpath, "notes": str(notes)},
                    capture_output=True,
                    text=True,
                    check=True,
                )
                self.assertEqual(result.stdout.rstrip(), expected, result.stderr)


class RegistrySourceMetadata(unittest.TestCase):
    def test_actual_cargo_metadata_schema_and_refusals(self) -> None:
        expected_sha = "a" * 40
        for metadata, valid in [
            ({"git": {"sha1": expected_sha}, "path_in_vcs": "crates/oxml"}, True),
            ({"git": {"sha1": expected_sha, "dirty": True}}, False),
            ({"git": {"sha1": "b" * 40}}, False),
            ({"git": {"sha": expected_sha}}, False),
            ({"git": {}}, False),
        ]:
            with self.subTest(metadata=metadata):
                result = subprocess.run(
                    ["bash", "-o", "pipefail", "-c", vcs_check_command()],
                    env={"PATH": os.environ.get("PATH", os.defpath),
                         "GITHUB_SHA": expected_sha},
                    input=json.dumps(metadata),
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertEqual(result.returncode == 0, valid, result.stderr)


if __name__ == "__main__":
    unittest.main()
