"""Tests for the keychain search list in release.yml's certificate import step.

Run from the repository root:
    python3 -m unittest discover -s .github/scripts -t .github/scripts
"""

from __future__ import annotations

import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKFLOW = REPO_ROOT / ".github/workflows/release.yml"
STEP_NAME = "Import Apple Developer ID certificates"

LOGIN_KEYCHAIN = "/Users/runner/Library/Keychains/login.keychain-db"
SPACED_KEYCHAIN = "/Users/runner/Library/Keychains/Team Keys.keychain-db"

SECURITY_STUB = """#!/bin/sh
set -eu
count=$(ls "$SECURITY_LOG_DIR" | wc -l | tr -d ' ')
printf '%s\\0' "$@" > "$SECURITY_LOG_DIR/$(printf '%04d' "$count")"
if [ "$#" -eq 3 ] && [ "$1" = "list-keychains" ] && [ "$2" = "-d" ] && [ "$3" = "user" ]; then
  cat "$SECURITY_FIXTURE"
fi
"""

OPENSSL_STUB = """#!/bin/sh
echo 0123456789abcdef
"""


class StepNotFoundError(LookupError):
    pass


def read_step_script(workflow: pathlib.Path, step_name: str) -> str:
    lines = workflow.read_text().splitlines()
    header = f"- name: {step_name}"
    start = next((i for i, line in enumerate(lines) if line.strip() == header), None)
    if start is None:
        raise StepNotFoundError(f"{workflow.name} has no step named {step_name!r}")
    step_indent = len(lines[start]) - len(lines[start].lstrip())
    key_indent = step_indent + 2
    for index in range(start + 1, len(lines)):
        line = lines[index]
        indent = len(line) - len(line.lstrip())
        if line.strip() and indent <= step_indent:
            break
        if indent == key_indent and line.strip() == "run: |":
            block: list[str] = []
            for body in lines[index + 1 :]:
                if body.strip() and len(body) - len(body.lstrip()) <= key_indent:
                    break
                block.append(body)
            while block and not block[-1].strip():
                block.pop()
            block_indent = min(len(b) - len(b.lstrip()) for b in block if b.strip())
            return "\n".join(b[block_indent:] for b in block) + "\n"
    raise StepNotFoundError(f"step {step_name!r} in {workflow.name} has no 'run: |' block")


def available_bash_paths() -> list[str]:
    candidates = [shutil.which("bash"), "/bin/bash"]
    resolved: dict[str, str] = {}
    for candidate in candidates:
        if candidate and os.path.exists(candidate):
            resolved.setdefault(os.path.realpath(candidate), candidate)
    if not resolved:
        raise FileNotFoundError("no bash found to run the release step with")
    return list(resolved.values())


class KeychainSearchListTests(unittest.TestCase):
    def setUp(self) -> None:
        self.script = read_step_script(WORKFLOW, STEP_NAME)
        self.tmp = pathlib.Path(tempfile.mkdtemp(prefix="writ-keychain-step-"))
        self.addCleanup(shutil.rmtree, self.tmp)

    def make_env(self, work: pathlib.Path, existing_keychains: list[str]) -> dict[str, str]:
        stubs = work / "stubs"
        stubs.mkdir()
        for name, body in (("security", SECURITY_STUB), ("openssl", OPENSSL_STUB)):
            stub = stubs / name
            stub.write_text(body)
            stub.chmod(0o755)
        log_dir = work / "security-calls"
        log_dir.mkdir()
        fixture = work / "list-keychains.txt"
        fixture.write_text("".join(f'    "{path}"\n' for path in existing_keychains))
        runner_temp = work / "runner-temp"
        runner_temp.mkdir()
        env = {
            "PATH": f"{stubs}:/usr/bin:/bin",
            "HOME": str(work),
            "RUNNER_TEMP": str(runner_temp),
            "SECURITY_LOG_DIR": str(log_dir),
            "SECURITY_FIXTURE": str(fixture),
        }
        for tool in ("security", "openssl"):
            self.assertEqual(
                shutil.which(tool, path=env["PATH"]),
                str(stubs / tool),
                f"{tool} does not resolve to the stub; refusing to run the step",
            )
        return env

    def security_calls(self, env: dict[str, str]) -> list[list[str]]:
        log_dir = pathlib.Path(env["SECURITY_LOG_DIR"])
        return [
            record.read_bytes().decode().split("\0")[:-1]
            for record in sorted(log_dir.iterdir())
        ]

    def run_step(self, bash: str, existing_keychains: list[str]) -> tuple[str, list[str]]:
        work = pathlib.Path(tempfile.mkdtemp(dir=self.tmp))
        env = self.make_env(work, existing_keychains)
        script = work / "step.sh"
        script.write_text(self.script)
        result = subprocess.run(
            [bash, "-e", str(script)],
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, f"step failed under {bash}: {result.stderr}")
        set_calls = [
            call
            for call in self.security_calls(env)
            if call[:1] in (["list-keychain"], ["list-keychains"]) and "-s" in call
        ]
        self.assertEqual(len(set_calls), 1, f"expected one search-list update, got {set_calls}")
        call = set_calls[0]
        self.assertEqual(call[1:4], ["-d", "user", "-s"])
        build_keychain = f"{env['RUNNER_TEMP']}/build.keychain-db"
        return build_keychain, call[4:]

    def test_step_script_is_found_and_carries_no_expressions(self) -> None:
        self.assertIn("security list-keychain", self.script)
        self.assertNotIn("${{", self.script)

    def test_search_list_keeps_each_existing_keychain_as_one_argument(self) -> None:
        for bash in available_bash_paths():
            with self.subTest(bash=bash):
                build_keychain, search_list = self.run_step(bash, [LOGIN_KEYCHAIN, SPACED_KEYCHAIN])
                self.assertEqual(search_list, [build_keychain, LOGIN_KEYCHAIN, SPACED_KEYCHAIN])

    def test_search_list_holds_the_build_keychain_when_none_exist(self) -> None:
        for bash in available_bash_paths():
            with self.subTest(bash=bash):
                build_keychain, search_list = self.run_step(bash, [])
                self.assertEqual(search_list, [build_keychain])

    @unittest.skipUnless(shutil.which("shellcheck"), "shellcheck is not installed")
    def test_step_script_passes_shellcheck(self) -> None:
        result = subprocess.run(
            ["shellcheck", "--norc", "--shell=bash", "-"],
            input=self.script,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
