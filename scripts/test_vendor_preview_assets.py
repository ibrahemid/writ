"""Tests for scripts/vendor-preview-assets.py against a fixture repository.

Run from the repository root:
    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import pathlib
import sys
import tempfile
import unittest

SCRIPT_PATH = pathlib.Path(__file__).with_name("vendor-preview-assets.py")
SPEC = importlib.util.spec_from_file_location("vendor_preview_assets", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Could not load {SCRIPT_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)

FONTS = ("KaTeX_Main-Regular.woff2", "KaTeX_Math-Italic.woff2")


def notices(katex_name: str = "KaTeX 0.16.22", fonts: tuple[str, ...] = FONTS) -> str:
    katex_files = ["src-tauri/assets/katex/katex.min.js", "src-tauri/assets/katex/katex.min.css"]
    katex_files += [f"src-tauri/assets/katex/fonts/{font}" for font in fonts]
    listed = ", ".join(f'"{name}"' for name in katex_files)
    return (
        f'[[bundled]]\nname = "{katex_name}"\nlicence = "MIT"\nfile = "katex.txt"\nfiles = [{listed}]\n'
        'sources = ["x"]\n\n'
        '[[bundled]]\nname = "Mermaid 11.15.0"\nlicence = "MIT"\nfile = "mermaid.txt"\n'
        'files = ["src-tauri/assets/mermaid/mermaid.min.js"]\nsources = ["x"]\n'
    )


class VendorPreviewAssetsTests(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="writ-vendor-assets-test-")
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.write("package.json", json.dumps({"devDependencies": {"katex": "0.16.22", "mermaid": "11.15.0"}}))
        self.write("node_modules/katex/package.json", json.dumps({"version": "0.16.22"}))
        self.write("node_modules/katex/dist/katex.min.js", "katex();")
        self.write("node_modules/katex/dist/katex.min.css", ".katex{}")
        for font in FONTS:
            self.write(f"node_modules/katex/dist/fonts/{font}", font)
        self.write("node_modules/katex/dist/fonts/KaTeX_Main-Regular.ttf", "not shipped")
        self.write("node_modules/mermaid/package.json", json.dumps({"version": "11.15.0"}))
        self.write("node_modules/mermaid/dist/mermaid.min.js", "mermaid();")
        self.write("licences/notices.toml", notices())
        self.write("src-tauri/assets/katex/init.js", "own file")

    def write(self, relative: str, text: str) -> pathlib.Path:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        return path

    def run_script(self, *args: str) -> tuple[int, str]:
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr), contextlib.redirect_stdout(io.StringIO()):
            code = MODULE.main(list(args), root=self.root)
        return code, stderr.getvalue()

    def test_copy_mirrors_the_packages_and_check_then_passes(self) -> None:
        self.assertEqual(self.run_script(), (0, ""))
        assets = self.root / "src-tauri/assets"
        self.assertEqual((assets / "mermaid/mermaid.min.js").read_text(), "mermaid();")
        self.assertEqual(sorted(p.name for p in (assets / "katex/fonts").iterdir()), sorted(FONTS))
        self.assertEqual((assets / "katex/init.js").read_text(), "own file")
        self.assertEqual(self.run_script("--check"), (0, ""))

    def test_check_refuses_a_copy_that_differs_from_its_package(self) -> None:
        self.run_script()
        self.write("src-tauri/assets/mermaid/mermaid.min.js", "patched();")
        code, err = self.run_script("--check")
        self.assertEqual(code, 1)
        self.assertIn("src-tauri/assets/mermaid/mermaid.min.js differs from mermaid 11.15.0", err)

    def test_check_refuses_a_missing_copy(self) -> None:
        code, err = self.run_script("--check")
        self.assertEqual(code, 1)
        self.assertIn("src-tauri/assets/katex/katex.min.js is missing", err)

    def test_a_font_the_package_dropped_is_flagged_and_copy_removes_it(self) -> None:
        self.run_script()
        stale = self.write("src-tauri/assets/katex/fonts/KaTeX_Old.woff2", "old")
        code, err = self.run_script("--check")
        self.assertEqual(code, 1)
        self.assertIn("KaTeX_Old.woff2 is not shipped by katex 0.16.22", err)
        self.assertEqual(self.run_script()[0], 0)
        self.assertFalse(stale.exists())

    def test_a_package_bump_needs_the_notices_to_follow(self) -> None:
        self.write("package.json", json.dumps({"devDependencies": {"katex": "0.16.23", "mermaid": "11.15.0"}}))
        self.write("node_modules/katex/package.json", json.dumps({"version": "0.16.23"}))
        code, err = self.run_script()
        self.assertEqual(code, 1)
        self.assertIn("names 'KaTeX 0.16.22', expected 'KaTeX 0.16.23'", err)

    def test_the_notices_must_list_every_copied_font(self) -> None:
        self.write("licences/notices.toml", notices(fonts=FONTS[:1]))
        code, err = self.run_script()
        self.assertEqual(code, 1)
        self.assertIn(f"leaves src-tauri/assets/katex/fonts/{FONTS[1]} out of KaTeX 0.16.22", err)

    def test_a_range_instead_of_a_pin_is_refused(self) -> None:
        self.write("package.json", json.dumps({"devDependencies": {"katex": "^0.16.22", "mermaid": "11.15.0"}}))
        code, err = self.run_script()
        self.assertEqual(code, 1)
        self.assertIn("pins katex ^0.16.22 but node_modules holds 0.16.22", err)

    def test_the_repository_copies_match_the_installed_packages(self) -> None:
        repo = SCRIPT_PATH.parent.parent
        if not (repo / "node_modules" / "mermaid").is_dir():
            self.skipTest("node_modules is not installed")
        self.assertEqual(MODULE.problems(repo), [])


if __name__ == "__main__":
    unittest.main()
