"""Tests for the push deploy skip in site.yml.

Run from the repository root:
    python3 -m unittest discover -s .github/scripts -t .github/scripts
"""

from __future__ import annotations

import pathlib
import re
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]

SKIP = (
    "github.event_name != 'push' || "
    "!startsWith(github.event.head_commit.message, 'chore(release): bump version to v')"
)


class SiteDeploySkipTests(unittest.TestCase):
    def setUp(self) -> None:
        self.site = (REPO_ROOT / ".github/workflows/site.yml").read_text()
        self.bump = (REPO_ROOT / ".github/workflows/bump-version.yml").read_text()

    def test_only_the_deploy_job_carries_the_skip(self) -> None:
        build, separator, deploy = self.site.partition("\n  deploy:\n")

        self.assertTrue(separator, "site.yml has no deploy job")
        self.assertIn(f'    if: "{SKIP}"\n', deploy)
        self.assertNotIn("head_commit", build)

    def test_every_bump_commit_subject_starts_with_the_skipped_prefix(self) -> None:
        prefix = re.search(r"startsWith\(github\.event\.head_commit\.message, '([^']+)'\)", SKIP).group(1)
        subjects = re.findall(r'git commit -m "([^"]+)"', self.bump) + re.findall(r'--title "([^"]+)"', self.bump)

        self.assertEqual(len(subjects), 3, subjects)
        for subject in subjects:
            with self.subTest(subject=subject):
                self.assertTrue(subject.replace("$NEW_VERSION", "0.6.0").startswith(prefix))


if __name__ == "__main__":
    unittest.main()
