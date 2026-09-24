import importlib.util
import pathlib
import unittest


SCRIPT_PATH = pathlib.Path(__file__).with_name("packaging_bump_aur.py")
SPEC = importlib.util.spec_from_file_location("packaging_bump_aur", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Could not load {SCRIPT_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

REPO_PKGBUILD = SCRIPT_PATH.parent.parent / "packaging/aur/writ-bin/PKGBUILD"
SHA = "a" * 64


class RewritePkgbuildTests(unittest.TestCase):
    def test_rewrites_version_checksum_and_description(self) -> None:
        stale = REPO_PKGBUILD.read_text().replace(
            f'pkgdesc="{MODULE.PACKAGE_DESCRIPTION}"', 'pkgdesc="Old scratchpad copy"'
        )

        result = MODULE.rewrite_pkgbuild(stale, "9.9.9", SHA)

        self.assertIn("pkgver=9.9.9\n", result)
        self.assertIn("pkgrel=1\n", result)
        self.assertIn(f'pkgdesc="{MODULE.PACKAGE_DESCRIPTION}"\n', result)
        self.assertIn(f"sha256sums=('{SHA}'", result)
        self.assertNotIn("scratchpad", result)

    def test_refuses_a_pkgbuild_without_a_description(self) -> None:
        text = "pkgver=1.0.0\npkgrel=2\nsha256sums=('x'\n 'SKIP')\n"
        with self.assertRaisesRegex(MODULE.PkgbuildFormatError, "pkgdesc"):
            MODULE.rewrite_pkgbuild(text, "1.0.1", SHA)

    def test_description_matches_the_committed_manifests(self) -> None:
        root = SCRIPT_PATH.parent.parent
        self.assertIn(f'pkgdesc="{MODULE.PACKAGE_DESCRIPTION}"', REPO_PKGBUILD.read_text())
        self.assertIn(
            f"pkgdesc = {MODULE.PACKAGE_DESCRIPTION}\n",
            (root / "packaging/aur/writ-bin/.SRCINFO").read_text(),
        )


if __name__ == "__main__":
    unittest.main()
