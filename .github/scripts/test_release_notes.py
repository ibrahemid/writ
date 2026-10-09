"""Tests for release_notes.py.

Run from the repository root:
    python3 -m unittest discover -s .github/scripts -t .github/scripts
"""

from __future__ import annotations

import contextlib
import io
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import release_notes  # noqa: E402

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]

ISOLATED_GIT_ENV = {
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_AUTHOR_NAME": "Writ Tests",
    "GIT_AUTHOR_EMAIL": "tests@example.com",
    "GIT_COMMITTER_NAME": "Writ Tests",
    "GIT_COMMITTER_EMAIL": "tests@example.com",
}


class FixtureRepo:
    def __init__(self, root: pathlib.Path) -> None:
        self.root = root
        self.clock = 1_760_000_000
        self.git("init", "--quiet", "--initial-branch=main")

    def git(self, *args: str) -> str:
        env = {**os.environ, **ISOLATED_GIT_ENV}
        env["GIT_AUTHOR_DATE"] = env["GIT_COMMITTER_DATE"] = f"{self.clock} +0000"
        result = subprocess.run(
            ["git", "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false", *args],
            cwd=self.root,
            env=env,
            capture_output=True,
            text=True,
            check=True,
        )
        return result.stdout

    def commit(self, subject: str, tag: str | None = None) -> str:
        self.clock += 60
        self.git("commit", "--quiet", "--allow-empty", "--message", subject)
        if tag:
            self.git("tag", tag)
        return self.git("rev-parse", "--short", "HEAD").strip()


class ReleaseNotesRepoTest(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        environment = mock.patch.dict(os.environ, ISOLATED_GIT_ENV)
        environment.start()
        self.addCleanup(environment.stop)
        self.repo = FixtureRepo(pathlib.Path(tmp.name))

    def notes(self, tag: str) -> str:
        return release_notes.release_notes(tag, "ibrahemid/writ", self.repo.root)


class PreviousTagTests(ReleaseNotesRepoTest):
    def setUp(self) -> None:
        super().setUp()
        self.repo.commit("feat: first release", tag="v0.4.0")
        self.repo.commit("fix(core): keep the cursor on save")
        self.repo.commit("feat(ui): add the apps panel", tag="v0.5.0-1")
        self.repo.commit("docs: describe the apps panel")
        self.repo.commit("chore(release): bump version to v0.5.0", tag="v0.5.0")

    def test_stable_tag_skips_candidates_back_to_the_previous_stable_tag(self) -> None:
        self.assertEqual(release_notes.previous_tag("v0.5.0", self.repo.root), "v0.4.0")

    def test_candidate_takes_the_previous_tag_of_either_kind(self) -> None:
        self.repo.commit("fix: second candidate fix", tag="v0.5.1-1")
        self.repo.commit("fix: third candidate fix", tag="v0.5.1-2")

        self.assertEqual(release_notes.previous_tag("v0.5.0-1", self.repo.root), "v0.4.0")
        self.assertEqual(release_notes.previous_tag("v0.5.1-1", self.repo.root), "v0.5.0")
        self.assertEqual(release_notes.previous_tag("v0.5.1-2", self.repo.root), "v0.5.1-1")

    def test_stable_tag_after_a_stable_tag_and_its_candidate(self) -> None:
        self.repo.commit("fix: drop gesture", tag="v0.5.1-1")
        self.repo.commit("chore(release): bump version to v0.5.1", tag="v0.5.1")

        self.assertEqual(release_notes.previous_tag("v0.5.1", self.repo.root), "v0.5.0")

    def test_stable_notes_list_every_commit_since_the_previous_stable_tag(self) -> None:
        notes = self.notes("v0.5.0")

        self.assertIn("- feat(ui): add the apps panel (", notes)
        self.assertIn("- fix(core): keep the cursor on save (", notes)
        self.assertIn("- docs: describe the apps panel (", notes)
        self.assertIn("- chore(release): bump version to v0.5.0 (", notes)
        self.assertNotIn("first release", notes)
        self.assertTrue(
            notes.endswith(
                "**Full Changelog**: https://github.com/ibrahemid/writ/compare/v0.4.0...v0.5.0\n"
            )
        )

    def test_candidate_notes_keep_the_range_since_the_previous_tag(self) -> None:
        notes = self.notes("v0.5.0-1")

        self.assertIn("- feat(ui): add the apps panel (", notes)
        self.assertIn("- fix(core): keep the cursor on save (", notes)
        self.assertNotIn("describe the apps panel", notes)
        self.assertIn("compare/v0.4.0...v0.5.0-1\n", notes)


class FirstReleaseTests(ReleaseNotesRepoTest):
    def test_lists_every_commit_up_to_the_tag_and_no_compare_link(self) -> None:
        self.repo.commit("feat: start")
        self.repo.commit("fix: early fix", tag="v0.1.0")
        self.repo.commit("feat: after the tag")

        notes = self.notes("v0.1.0")

        self.assertIsNone(release_notes.previous_tag("v0.1.0", self.repo.root))
        self.assertIn("- feat: start (", notes)
        self.assertIn("- fix: early fix (", notes)
        self.assertNotIn("after the tag", notes)
        self.assertNotIn("Full Changelog", notes)

    def test_tag_on_the_root_commit_has_no_previous_tag(self) -> None:
        self.repo.commit("feat: start", tag="v0.1.0")

        self.assertIsNone(release_notes.previous_tag("v0.1.0", self.repo.root))
        self.assertIn("- feat: start (", self.notes("v0.1.0"))

    def test_refuses_a_tag_that_does_not_exist(self) -> None:
        self.repo.commit("feat: start", tag="v0.1.0")

        with self.assertRaises(release_notes.ReleaseNotesError):
            self.notes("v9.9.9")


class RenderNotesTests(unittest.TestCase):
    def test_groups_entries_in_the_workflow_layout(self) -> None:
        lines = [
            "- chore(release): bump version to v0.5.1 (aaaaaaa)",
            "- fix(editor): keep typed text (bbbbbbb)",
            "- feat(mcp): list every text file (ccccccc)",
            "- feat!: breaking change (ddddddd)",
            "- fix: atomic config (eeeeeee)",
        ]

        notes = release_notes.render_notes(
            lines, tag="v0.5.1", previous="v0.5.0", repository="ibrahemid/writ"
        )

        self.assertEqual(
            notes,
            "## What's changed\n"
            "\n"
            "### Features\n"
            "- feat(mcp): list every text file (ccccccc)\n"
            "\n"
            "\n"
            "### Fixes\n"
            "- fix(editor): keep typed text (bbbbbbb)\n"
            "- fix: atomic config (eeeeeee)\n"
            "\n"
            "\n"
            "### Other\n"
            "- chore(release): bump version to v0.5.1 (aaaaaaa)\n"
            "- feat!: breaking change (ddddddd)\n"
            "\n"
            "\n"
            "**Full Changelog**: https://github.com/ibrahemid/writ/compare/v0.5.0...v0.5.1\n",
        )

    def test_empty_groups_say_none_and_no_previous_tag_drops_the_compare_link(self) -> None:
        notes = release_notes.render_notes(
            [], tag="v0.1.0", previous=None, repository="ibrahemid/writ"
        )

        self.assertEqual(
            notes,
            "## What's changed\n"
            "\n"
            "### Features\n"
            "- None\n"
            "\n"
            "\n"
            "### Fixes\n"
            "- None\n"
            "\n"
            "\n"
            "### Other\n"
            "- None\n"
            "\n",
        )


class MainTests(ReleaseNotesRepoTest):
    def run_main(self, **env: str) -> tuple[int, str, str]:
        stdout, stderr = io.StringIO(), io.StringIO()
        previous_cwd = os.getcwd()
        os.chdir(self.repo.root)
        self.addCleanup(os.chdir, previous_cwd)
        with mock.patch.dict(os.environ, env, clear=False):
            for name in ("TAG", "REPO"):
                if name not in env:
                    os.environ.pop(name, None)
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                code = release_notes.main()
        return code, stdout.getvalue(), stderr.getvalue()

    def test_writes_the_notes_to_stdout_and_the_range_to_stderr(self) -> None:
        self.repo.commit("feat: first", tag="v0.4.0")
        self.repo.commit("feat: candidate", tag="v0.5.0-1")
        self.repo.commit("fix: stable", tag="v0.5.0")

        code, stdout, stderr = self.run_main(TAG="v0.5.0", REPO="ibrahemid/writ")

        self.assertEqual(code, 0)
        self.assertEqual(stdout, self.notes("v0.5.0"))
        self.assertIn("v0.4.0..v0.5.0", stderr)

    def test_fails_without_a_tag(self) -> None:
        code, stdout, stderr = self.run_main(REPO="ibrahemid/writ")

        self.assertEqual(code, 1)
        self.assertEqual(stdout, "")
        self.assertIn("TAG", stderr)

    def test_fails_on_an_unknown_tag(self) -> None:
        self.repo.commit("feat: start", tag="v0.1.0")

        code, stdout, stderr = self.run_main(TAG="v9.9.9", REPO="ibrahemid/writ")

        self.assertEqual(code, 1)
        self.assertEqual(stdout, "")
        self.assertIn("v9.9.9", stderr)


class ReleaseWorkflowTests(unittest.TestCase):
    def test_release_workflow_generates_the_notes_with_this_script(self) -> None:
        workflow = (REPO_ROOT / ".github/workflows/release.yml").read_text()

        self.assertTrue(
            "python3 .github/scripts/release_notes.py > CHANGELOG_RELEASE.md" in workflow,
            "release.yml does not generate the notes with release_notes.py",
        )
        self.assertFalse("git describe" in workflow, "release.yml still picks the previous tag itself")


if __name__ == "__main__":
    unittest.main()
