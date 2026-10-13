"""Every build CI, the release and the preflight run uses the tested lockfile
on the pinned compiler.

Run from the repository root:
    python3 -m unittest discover -s .github/scripts -t .github/scripts
"""

from __future__ import annotations

import pathlib
import re
import tomllib
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKFLOWS = REPO_ROOT / ".github/workflows"
PREFLIGHT = REPO_ROOT / "scripts/release-preflight.sh"

# Subcommands that leave Cargo.lock alone by design, or never read it.
EXEMPT_SUBCOMMANDS = {"fmt", "update"}
# Subcommands that take no --locked flag. Each must follow a
# `cargo metadata --locked` in the same step, which fails first when the
# lockfile would change.
GUARDED_SUBCOMMANDS = {"cyclonedx"}
CARGO_CALL = re.compile(r"(?<![\w-])cargo\s+(?:\+\S+\s+)?([a-z][\w-]*)([^\n]*)")


def command_lines(text: str) -> list[str]:
    """Lines with shell continuations joined and comments dropped."""
    joined = re.sub(r"\\\n\s*", " ", text)
    return [line for line in joined.splitlines() if not line.lstrip().startswith("#")]


def unlocked_cargo_calls(text: str) -> list[str]:
    calls = []
    for line in command_lines(text):
        stripped = line.strip()
        # Step names, tool pins and the preflight's printed labels name cargo
        # without running it.
        if stripped.startswith(("- name:", "name:", "tool:", "uses:", "- uses:", "step ", "warn ", "echo ")):
            continue
        for match in CARGO_CALL.finditer(line):
            subcommand, rest = match.group(1), match.group(2)
            if subcommand in EXEMPT_SUBCOMMANDS | GUARDED_SUBCOMMANDS:
                continue
            if subcommand == "metadata" and "--no-deps" in rest:
                continue
            if "--locked" not in rest:
                calls.append(stripped)
    return calls


def guarded_call(line: str) -> bool:
    return any(match.group(1) in GUARDED_SUBCOMMANDS for match in CARGO_CALL.finditer(line))


def workflow_steps(text: str) -> list[str]:
    steps: list[list[str]] = []
    for line in text.splitlines():
        if re.match(r"^\s+- (?:name|uses):", line):
            steps.append([])
        if steps:
            steps[-1].append(line)
    return ["\n".join(step) for step in steps]


class LockedBuildTests(unittest.TestCase):
    def workflow_texts(self) -> dict[str, str]:
        return {path.name: path.read_text() for path in sorted(WORKFLOWS.glob("*.yml"))}

    def test_every_cargo_call_in_a_workflow_is_locked(self) -> None:
        for name, text in self.workflow_texts().items():
            with self.subTest(workflow=name):
                self.assertEqual(unlocked_cargo_calls(text), [])

    def test_every_cargo_call_in_the_preflight_is_locked(self) -> None:
        self.assertEqual(unlocked_cargo_calls(PREFLIGHT.read_text()), [])

    def test_the_release_build_hands_locked_to_cargo(self) -> None:
        release = (WORKFLOWS / "release.yml").read_text()
        builds = [line.strip() for line in command_lines(release) if "tauri build" in line and "name:" not in line]
        self.assertEqual(len(builds), 1, builds)
        self.assertTrue(builds[0].endswith(" -- --locked"), builds[0])

    def test_a_guarded_subcommand_follows_a_locked_metadata_call(self) -> None:
        for name, text in self.workflow_texts().items():
            for step in workflow_steps(text):
                calls = [line for line in command_lines(step) if guarded_call(line)]
                if not calls:
                    continue
                with self.subTest(workflow=name, call=calls[0].strip()):
                    lines = command_lines(step)
                    first_call = next(index for index, line in enumerate(lines) if guarded_call(line))
                    guards = [line for line in lines[:first_call] if "cargo metadata --locked" in line]
                    self.assertTrue(guards, "no `cargo metadata --locked` before it in the same step")

    def test_the_release_workflow_runs_a_guarded_subcommand(self) -> None:
        release = (WORKFLOWS / "release.yml").read_text()
        self.assertTrue(any(guarded_call(line) for line in command_lines(release)))

    def test_the_preflight_build_hands_locked_to_cargo(self) -> None:
        builds = [
            line for line in command_lines(PREFLIGHT.read_text()) if "npx tauri build" in line
        ]
        self.assertEqual(len(builds), 1, builds)
        self.assertTrue(builds[0].rstrip().endswith("-- --locked"), builds[0])

    def test_a_missing_flag_is_reported(self) -> None:
        self.assertEqual(
            unlocked_cargo_calls("        run: cargo test --workspace\n"),
            ["run: cargo test --workspace"],
        )
        self.assertEqual(unlocked_cargo_calls("  cargo build -p writ-cli \\\n    --locked\n"), [])


class PinnedToolchainTests(unittest.TestCase):
    def test_the_toolchain_file_pins_an_exact_release(self) -> None:
        toolchain = tomllib.loads((REPO_ROOT / "rust-toolchain.toml").read_text())["toolchain"]
        self.assertRegex(toolchain["channel"], r"^\d+\.\d+\.\d+$")
        self.assertIn("clippy", toolchain["components"])
        self.assertIn("rustfmt", toolchain["components"])

    def test_no_workflow_installs_a_floating_toolchain(self) -> None:
        for path in sorted(WORKFLOWS.glob("*.yml")):
            text = path.read_text()
            with self.subTest(workflow=path.name):
                self.assertNotIn("dtolnay/rust-toolchain", text)
                self.assertIsNone(re.search(r"toolchain:\s*stable", text))
                self.assertIsNone(re.search(r"rustup (?:default|toolchain install) stable", text))


if __name__ == "__main__":
    unittest.main()
