"""Structural checks on packages.yml: the bumped AUR package is built on Arch.

Run from the repository root:
    python3 -m unittest discover -s .github/scripts -p 'test_*.py'
"""

from __future__ import annotations

import importlib.util
import pathlib
import unittest

FIXTURE_PATH = pathlib.Path(__file__).with_name("test_release_workflow.py")
SPEC = importlib.util.spec_from_file_location("test_release_workflow_fixture", FIXTURE_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Could not load {FIXTURE_PATH}")
FIXTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FIXTURE)

PACKAGES = FIXTURE.REPO_ROOT / ".github/workflows/packages.yml"
ARTIFACT = "name: aur-pkgbuild"


class AurBuildTests(unittest.TestCase):
    def setUp(self) -> None:
        self.jobs = FIXTURE.workflow_jobs(PACKAGES.read_text())

    def step_index(self, steps: list[str], needle: str) -> int:
        matches = [index for index, step in enumerate(steps) if needle in step]
        self.assertEqual(len(matches), 1, f"expected one step with {needle!r}")
        return matches[0]

    def test_the_bump_job_hands_on_the_bumped_pkgbuild(self) -> None:
        steps = FIXTURE.job_steps(self.jobs["bump"])
        upload = self.step_index(steps, ARTIFACT)

        self.assertTrue((FIXTURE.step_uses(steps[upload]) or "").startswith("actions/upload-artifact@"))
        self.assertIn("path: packaging/aur/writ-bin/PKGBUILD", steps[upload])
        self.assertIn("if-no-files-found: error", steps[upload])
        self.assertLess(self.step_index(steps, "packaging_bump_aur.py"), upload)

    def test_the_arch_job_builds_the_bumped_pkgbuild_in_an_arch_container_with_no_token_scopes(self) -> None:
        job = self.jobs["aur-build"]

        self.assertEqual(FIXTURE.job_field(job, "needs"), "bump")
        self.assertEqual(FIXTURE.job_field(job, "container"), "archlinux:base-devel")
        self.assertEqual(FIXTURE.job_permissions(job, {"contents": "write"}), {})
        steps = FIXTURE.job_steps(job)
        download = self.step_index(steps, ARTIFACT)
        self.assertTrue((FIXTURE.step_uses(steps[download]) or "").startswith("actions/download-artifact@"))
        self.assertLess(download, self.step_index(steps, "makepkg"))

    def test_makepkg_runs_as_a_user_who_may_only_run_pacman_as_root(self) -> None:
        steps = FIXTURE.job_steps(self.jobs["aur-build"])
        script = FIXTURE.step_script(steps[self.step_index(steps, "makepkg")])

        self.assertIn("useradd --create-home builder\n", script)
        self.assertIn("printf 'builder ALL=(root) NOPASSWD: /usr/bin/pacman\\n' > /etc/sudoers.d/builder\n", script)
        makepkg = [line for line in script.splitlines() if "makepkg" in line]
        self.assertEqual(
            makepkg, ["sudo -u builder bash -c 'cd /home/builder/package && makepkg --syncdeps --noconfirm'"]
        )

    def test_the_built_package_must_hold_both_commands(self) -> None:
        steps = FIXTURE.job_steps(self.jobs["aur-build"])
        check = self.step_index(steps, "pacman -Qlpq")

        self.assertLess(self.step_index(steps, "makepkg"), check)
        script = FIXTURE.step_script(steps[check])
        self.assertIn("for command in /usr/bin/jotra /usr/bin/writ; do\n", script)
        self.assertIn('if ! grep -qxF "$command" <<<"$LISTING"; then\n', script)


if __name__ == "__main__":
    unittest.main()
