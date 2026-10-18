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


def step_command(name: str) -> str:
    workflow = Path(__file__).resolve().parent.parent / ".github/workflows/release.yml"
    step = workflow.read_text().split(f"      - name: {name}\n", 1)[1]
    body = step.split("        run: |\n", 1)[1]
    lines = []
    for line in body.splitlines():
        if line.strip() and not line.startswith("          "):
            break
        lines.append(line[10:])
    return "\n".join(lines)


def run_registry_probe(
    name: str, curl_exit: int
) -> tuple[subprocess.CompletedProcess[str], list[str], str]:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        calls = root / "calls"
        output = root / "output"
        for name_, source in {
            "curl": '#!/bin/sh\nprintf "%s\\n" "$*" >> "$TEST_CALLS"\n'
                    'exit "$TEST_CURL_EXIT"\n',
            "sleep": "#!/bin/sh\nexit 0\n",
        }.items():
            tool = root / name_
            tool.write_text(source)
            tool.chmod(0o755)
        result = subprocess.run(
            ["bash", "-e", "-o", "pipefail", "-c", step_command(name)],
            cwd=root, capture_output=True, text=True, check=False, timeout=10,
            env={"PATH": str(root) + os.pathsep + os.defpath,
                 "GITHUB_REF_NAME": "v0.0.11", "GITHUB_OUTPUT": str(output),
                 "TEST_CALLS": str(calls), "TEST_CURL_EXIT": str(curl_exit)},
        )
        return result, calls.read_text().splitlines(), (
            output.read_text() if output.exists() else ""
        )


class RegistryProbeBounds(unittest.TestCase):
    def assert_bounded(self, calls: list[str], count: int) -> None:
        self.assertEqual(len(calls), count)
        for call in calls:
            args = call.split()
            self.assertIn("--connect-timeout", args)
            self.assertEqual(args[args.index("--connect-timeout") + 1], "10")
            self.assertIn("--max-time", args)
            self.assertEqual(args[args.index("--max-time") + 1], "20")

    def test_publication_lookup_preserves_skip_and_retry_behavior(self) -> None:
        for code, expected in [(0, "skip=true\n"), (28, "skip=false\n")]:
            with self.subTest(curl_exit=code):
                result, calls, output = run_registry_probe(
                    "Is this version already published?", code
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(output, expected)
                self.assert_bounded(calls, 1)

    def test_index_success_stops_after_one_bounded_probe(self) -> None:
        result, calls, _ = run_registry_probe("Confirm it is on crates.io", 0)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("oxml 0.0.11 is live", result.stdout)
        self.assert_bounded(calls, 1)

    def test_index_exhaustion_fails_with_an_accurate_attempt_count(self) -> None:
        result, calls, _ = run_registry_probe("Confirm it is on crates.io", 28)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("did not appear after 30 checks", result.stdout)
        self.assertNotIn("within five minutes", result.stdout)
        self.assert_bounded(calls, 30)


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

    def test_package_sbom_and_checksums_step_executes(self) -> None:
        cmd = step_command("Package, SBOM and checksums").replace(
            "${{ needs.preflight.outputs.version }}", "0.0.11"
        )
        self.assertNotIn("cargo cyclonedx -p", cmd)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "crates/oxml").mkdir(parents=True)
            expected_sha = "a" * 40
            archive = root / "archive.crate"
            vcs_dir = root / "oxml-0.0.11"
            vcs_dir.mkdir()
            (vcs_dir / ".cargo_vcs_info.json").write_text(
                json.dumps({"git": {"sha1": expected_sha}})
            )
            subprocess.run(
                ["tar", "-czf", str(archive), "oxml-0.0.11/.cargo_vcs_info.json"],
                cwd=root, check=True,
            )
            cksum = subprocess.check_output(
                ["shasum", "-a", "256", str(archive)], text=True
            ).split()[0]
            curl_script = (
                "#!/bin/sh\n"
                'case "$*" in\n'
                '  *--output*) cp "$TEST_ARCHIVE" "dist/oxml-0.0.11.crate" ;;\n'
                f'  *) printf \'{{"version":{{"crate":"oxml","num":"0.0.11","checksum":"{cksum}"}}}}\' ;;\n'
                "esac\n"
            )
            cargo_script = (
                "#!/bin/sh\n"
                'for arg in "$@"; do [ "$arg" = "-p" ] && exit 2; done\n'
                'printf "{}" > crates/oxml/oxml-0.0.11.cdx.json\n'
            )
            sha256sum_script = (
                "#!/bin/sh\n"
                'if [ "${1:-}" = "--check" ]; then exec shasum -a 256 -c; '
                'else exec shasum -a 256 "$@"; fi\n'
            )
            for name, src in [
                ("curl", curl_script),
                ("cargo", cargo_script),
                ("sha256sum", sha256sum_script),
            ]:
                tool = root / name
                tool.write_text(src)
                tool.chmod(0o755)
            result = subprocess.run(
                ["bash", "-e", "-o", "pipefail", "-c", cmd],
                cwd=root, capture_output=True, text=True, check=False,
                env={
                    "PATH": str(root) + os.pathsep + os.environ.get("PATH", os.defpath),
                    "GITHUB_SHA": expected_sha,
                    "TEST_ARCHIVE": str(archive),
                },
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("oxml-0.0.11.cdx.json", result.stdout)
            self.assertIn("oxml-0.0.11.crate", result.stdout)


if __name__ == "__main__":
    unittest.main()
