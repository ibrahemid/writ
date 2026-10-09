import pathlib
import shutil
import subprocess
import unittest


REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent


def is_git_checkout() -> bool:
    if shutil.which("git") is None:
        return False
    result = subprocess.run(
        ["git", "rev-parse", "--is-inside-work-tree"],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
    )
    return result.returncode == 0 and result.stdout.strip() == "true"


@unittest.skipUnless(is_git_checkout(), "needs git and a checkout of the repository")
class TargetDirectoryIgnoreTests(unittest.TestCase):
    def matching_source(self, path: str) -> str:
        result = subprocess.run(
            ["git", "check-ignore", "--verbose", "--no-index", path],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            self.fail(f"{path} is not ignored")
        source, _, _ = result.stdout.partition(":")
        return source

    def test_root_build_trees_with_a_suffix_are_ignored_by_the_repository(self) -> None:
        for path in ("target-release-9.9.9/", "target-daily/", "target-shared/"):
            with self.subTest(path=path):
                self.assertEqual(self.matching_source(path), ".gitignore")

    def test_no_tracked_path_sits_under_an_ignored_build_tree(self) -> None:
        result = subprocess.run(
            ["git", "ls-files", "--", "target-*/"],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=True,
        )
        self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
