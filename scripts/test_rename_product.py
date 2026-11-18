import contextlib
import importlib.util
import io
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest


SCRIPT_PATH = pathlib.Path(__file__).with_name("rename_product.py")
SPEC = importlib.util.spec_from_file_location("rename_product", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Could not load {SCRIPT_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)
NAMES = MODULE.names

ALLOWLIST = (
    "docs/adr/0[0-5][0-9]-*.md -- history\n"
    "themes/writ-*.json :path -- kept ids\n"
    "Writ/legacy.txt -- legacy\n"
)

TREE = {
    "README.md": (
        "# Writ\n\n"
        "Writ is a text app. Run `writ --help`. Data lives in WRIT_DATA_DIR.\n"
        "See docs/adr/041-writ-is-a-text-app.md and themes/writ-classic.json.\n"
        "Bundle id com.writ.editor, theme writ-dark. Users rewrite and write.\n"
        "Source: https://github.com/ibrahemid/writ\n"
    ),
    "crates/writ-core/Cargo.toml": '[package]\nname = "writ-core"\n',
    "crates/writ-core/src/lib.rs": 'pub const SCHEME: &str = "writ://";\npub fn is_writ_command() -> bool { true }\n',
    "crates/writ-storage/src/lib.rs": "use writ_core::SCHEME;\n",
    "src/commands.ts": "export const isWritCommand = (name: string) => name.startsWith('writ');\n",
    "src/theme.css": ":root { --writ-accent: #000; }\n",
    "apple/WritOnDevice/WritOnDevice.swift": "struct WritOnDevice {}\n",
    "themes/writ-classic.json": '{"name": "Writ Classic"}\n',
    "docs/adr/041-writ-is-a-text-app.md": "# Writ is a text app\n",
    "Writ/legacy.txt": "Writ 0.1 data\n",
    "Writ/a.txt": "Writ\n",
    "Cargo.lock": 'name = "writ-core"\n',
    "assets/logo.bin": b"\x00writ\x00",
}

RENAMED_TREE = {
    "README.md": (
        "# Quill\n\n"
        "Quill is a text app. Run `quill --help`. Data lives in QUILL_DATA_DIR.\n"
        "See docs/adr/041-writ-is-a-text-app.md and themes/writ-classic.json.\n"
        "Bundle id com.writ.editor, theme writ-dark. Users rewrite and write.\n"
        "Source: https://github.com/ibrahemid/writ\n"
    ),
    "crates/quill-core/Cargo.toml": '[package]\nname = "quill-core"\n',
    "crates/quill-core/src/lib.rs": 'pub const SCHEME: &str = "quill://";\npub fn is_quill_command() -> bool { true }\n',
    "crates/quill-storage/src/lib.rs": "use quill_core::SCHEME;\n",
    "src/commands.ts": "export const isQuillCommand = (name: string) => name.startsWith('quill');\n",
    "src/theme.css": ":root { --quill-accent: #000; }\n",
    "apple/QuillOnDevice/QuillOnDevice.swift": "struct QuillOnDevice {}\n",
    "themes/writ-classic.json": '{"name": "Quill Classic"}\n',
    "docs/adr/041-writ-is-a-text-app.md": "# Writ is a text app\n",
    "Writ/legacy.txt": "Writ 0.1 data\n",
    "Quill/a.txt": "Quill\n",
    "Cargo.lock": 'name = "writ-core"\n',
    "assets/logo.bin": b"\x00writ\x00",
}


def write_tree(root, files):
    for relative, content in files.items():
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, bytes):
            path.write_bytes(content)
        else:
            path.write_text(content, encoding="utf-8")


def read_tree(root):
    tree = {}
    for directory, subdirectories, filenames in os.walk(root):
        subdirectories[:] = [name for name in subdirectories if name != ".git"]
        for name in filenames:
            path = pathlib.Path(directory) / name
            data = path.read_bytes()
            tree[path.relative_to(root).as_posix()] = data if b"\0" in data else data.decode("utf-8")
    return tree


def init_git(root):
    subprocess.run(["git", "init", "-q"], cwd=root, check=True)


class TreeTestCase(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.base = pathlib.Path(self.tmp.name)
        self.root = self.base / "tree"
        self.root.mkdir()
        self.allowlist_path = self.base / "allowlist.txt"
        self.allowlist_path.write_text(ALLOWLIST, encoding="utf-8")

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def plan(self, name, old="Writ", stage="app", root=None):
        root = root or self.root
        files = NAMES.list_files(root)
        scanner = NAMES.NameScanner(NAMES.get_name_forms(old), stage, NAMES.load_allowlist(self.allowlist_path), files)
        return files, MODULE.plan_rename(root, files, scanner, NAMES.get_name_forms(name))

    def rename(self, name, old="Writ", stage="app", root=None):
        root = root or self.root
        files, plan = self.plan(name, old, stage, root)
        MODULE.apply_plan(root, files, plan)
        return plan

    def run_main(self, *args):
        output = io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
            code = MODULE.main([*args, "--root", str(self.root), "--allowlist", str(self.allowlist_path)])
        return code, output.getvalue().splitlines()


class RenameTests(TreeTestCase):
    def test_renames_paths_and_text_in_every_case(self) -> None:
        write_tree(self.root, TREE)
        plan = self.rename("Quill")
        self.assertEqual(read_tree(self.root), RENAMED_TREE)
        self.assertEqual((len(plan.moves), len(plan.edits), plan.replacement_count), (5, 9, 14))

    def test_renamed_directories_leave_nothing_behind(self) -> None:
        write_tree(self.root, TREE)
        self.rename("Quill")
        for directory in ("crates/writ-core", "crates/writ-storage", "apple/WritOnDevice"):
            with self.subTest(directory=directory):
                self.assertFalse((self.root / directory).exists())
        self.assertTrue((self.root / "Writ").is_dir())

    def test_a_directory_with_a_kept_file_is_split_per_file(self) -> None:
        write_tree(self.root, TREE)
        files, plan = self.plan("Quill")
        self.assertIn(("crates/writ-core", "crates/quill-core"), MODULE.get_directory_moves(files, plan.moves))
        self.assertNotIn("Writ", [old for old, _ in MODULE.get_directory_moves(files, plan.moves)])

    def test_a_second_pass_finds_nothing(self) -> None:
        write_tree(self.root, TREE)
        self.rename("Quill")
        _, plan = self.plan("Quill")
        self.assertTrue(plan.is_empty)

    def test_a_round_trip_through_another_name_matches_a_direct_rename(self) -> None:
        write_tree(self.root, TREE)
        self.rename("Quill")
        self.rename("Lumen", old="Quill")
        direct = self.base / "direct"
        direct.mkdir()
        write_tree(direct, TREE)
        self.rename("Lumen", root=direct)
        self.assertEqual(read_tree(self.root), read_tree(direct))

    def test_channels_stage_renames_the_slug_and_host(self) -> None:
        write_tree(self.root, {"install.sh": 'REPO="ibrahemid/writ"\nSITE="https://writ.ibrahemid.com"\nbrew tap ibrahemid/writ\n'})
        self.rename("Quill", stage="app")
        self.assertEqual(
            read_tree(self.root)["install.sh"],
            'REPO="ibrahemid/writ"\nSITE="https://writ.ibrahemid.com"\nbrew tap ibrahemid/writ\n',
        )
        self.rename("Quill", stage="channels")
        self.assertEqual(
            read_tree(self.root)["install.sh"],
            'REPO="ibrahemid/quill"\nSITE="https://quill.ibrahemid.com"\nbrew tap ibrahemid/writ\n',
        )

    def test_channels_stage_renames_only_the_slug_and_host(self) -> None:
        write_tree(
            self.root,
            {
                "src/help-links.ts": 'const DOCS = "https://writ.ibrahemid.com/docs";\nconst ISSUES = "https://github.com/ibrahemid/writ/issues";\nexport const APP = "Writ";\n',
                "crates/writ-core/Cargo.toml": 'name = "writ-core"\n',
            },
        )
        plan = self.rename("Quill", stage="channels")
        self.assertEqual(plan.moves, ())
        self.assertEqual(
            read_tree(self.root),
            {
                "src/help-links.ts": 'const DOCS = "https://quill.ibrahemid.com/docs";\nconst ISSUES = "https://github.com/ibrahemid/quill/issues";\nexport const APP = "Writ";\n',
                "crates/writ-core/Cargo.toml": 'name = "writ-core"\n',
            },
        )

    def test_generated_outputs_are_left_to_their_generators(self) -> None:
        for path in ("Cargo.lock", "src/styles/generated/tokens.css", "src-tauri/gen/schemas/capabilities.json"):
            with self.subTest(path=path):
                self.assertTrue(MODULE.is_generated(path))
        self.assertFalse(MODULE.is_generated("src/styles/app.css"))

    @unittest.skipUnless(shutil.which("git"), "git is not installed")
    def test_ignored_files_move_with_their_directory(self) -> None:
        write_tree(
            self.root,
            {".gitignore": "*.local\n", "crates/writ-core/src/lib.rs": "use writ_core;\n", "crates/writ-core/cache.local": "x"},
        )
        init_git(self.root)
        self.rename("Quill")
        self.assertEqual(
            read_tree(self.root),
            {".gitignore": "*.local\n", "crates/quill-core/src/lib.rs": "use quill_core;\n", "crates/quill-core/cache.local": "x"},
        )


class ConflictTests(TreeTestCase):
    def test_two_files_cannot_share_a_new_path(self) -> None:
        write_tree(self.root, {"notes/writ.txt": "a", "notes/quill.txt": "b"})
        with self.assertRaisesRegex(MODULE.PathConflictError, "would both become notes/quill.txt"):
            self.plan("Quill")

    @unittest.skipUnless(shutil.which("git"), "git is not installed")
    def test_an_unlisted_file_at_the_new_path_is_not_overwritten(self) -> None:
        write_tree(self.root, {".gitignore": "quill.txt\n", "notes/writ.txt": "a", "notes/quill.txt": "b"})
        init_git(self.root)
        with self.assertRaisesRegex(MODULE.PathConflictError, "already exists"):
            self.plan("Quill")

    def test_a_conflict_writes_nothing(self) -> None:
        tree = {"notes/writ.txt": "Writ", "notes/quill.txt": "b"}
        write_tree(self.root, tree)
        code, lines = self.run_main("--name", "Quill", "--stage", "app")
        self.assertEqual(code, 2)
        self.assertIn("would both become", lines[-1])
        self.assertEqual(read_tree(self.root), tree)


class NameValidationTests(unittest.TestCase):
    def test_refuses_the_old_name(self) -> None:
        with self.assertRaises(NAMES.InvalidNameError):
            MODULE.validate_new_name(NAMES.get_name_forms("Writ"), NAMES.get_name_forms("Writ"))

    def test_refuses_a_name_the_old_pattern_would_match_again(self) -> None:
        for name in ("Writ2", "Writ0"):
            with self.subTest(name=name):
                with self.assertRaises(NAMES.InvalidNameError):
                    MODULE.validate_new_name(NAMES.get_name_forms("Writ"), NAMES.get_name_forms(name))

    def test_accepts_an_unrelated_name(self) -> None:
        MODULE.validate_new_name(NAMES.get_name_forms("Writ"), NAMES.get_name_forms("Quill"))


class CommandLineTests(TreeTestCase):
    def test_check_reports_the_plan_and_writes_nothing(self) -> None:
        write_tree(self.root, TREE)
        code, lines = self.run_main("--name", "Quill", "--stage", "app", "--check")
        self.assertEqual(code, 1)
        self.assertIn("rename crates/writ-core/Cargo.toml -> crates/quill-core/Cargo.toml", lines)
        self.assertIn("edit README.md (4)", lines)
        self.assertEqual(lines[-1], "5 paths to rename, 9 files to edit, 14 replacements")
        self.assertEqual(read_tree(self.root), TREE)

    def test_check_passes_after_the_rename(self) -> None:
        write_tree(self.root, TREE)
        self.run_main("--name", "Quill", "--stage", "app")
        code, lines = self.run_main("--name", "Quill", "--stage", "app", "--check")
        self.assertEqual(code, 0)
        self.assertEqual(lines, ["0 paths to rename, 0 files to edit, 0 replacements"])

    def test_write_mode_renames_then_reports_what_the_guard_still_finds(self) -> None:
        write_tree(self.root, TREE)
        code, lines = self.run_main("--name", "Quill", "--stage", "app")
        self.assertEqual(code, 1)
        self.assertEqual(lines[0], "renamed 5 paths, edited 9 files, 14 replacements")
        self.assertEqual(lines[-2:], ["Cargo.lock:1:9 writ-core", "1 hit of Writ outside the allowlist"])
        self.assertEqual(read_tree(self.root), RENAMED_TREE)

    def test_an_invalid_name_is_an_error(self) -> None:
        write_tree(self.root, TREE)
        for name in ("quill", "Writ"):
            with self.subTest(name=name):
                code, lines = self.run_main("--name", name, "--stage", "app")
                self.assertEqual(code, 2)
                self.assertTrue(lines[-1].startswith("rename_product: "))
        self.assertEqual(read_tree(self.root), TREE)


class GeneratorTests(unittest.TestCase):
    def run_generators(self, generators, root):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            failures = MODULE.run_generators(root, generators)
        return failures, output.getvalue().splitlines()

    def test_reports_each_generator_outcome(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            failures, lines = self.run_generators(
                (
                    MODULE.Generator("out-a", "a build step"),
                    MODULE.Generator("out-b", "a tool", ("absent-tool",), ("absent-tool-for-rename-tests",)),
                    MODULE.Generator("out-c", "a file", (sys.executable, "-c", "pass"), (), ("absent.txt",)),
                    MODULE.Generator("out-d", "a script", (sys.executable, "-c", "pass")),
                    MODULE.Generator(
                        "out-e",
                        "a failing script",
                        (sys.executable, "-c", "import sys; sys.stderr.write('stopped\\n'); sys.exit(3)"),
                    ),
                ),
                root,
            )
        self.assertEqual(failures, ["out-e"])
        self.assertEqual(
            lines,
            [
                "pending: out-a <- a build step",
                "skipped: out-b <- a tool; needs absent-tool-for-rename-tests",
                "skipped: out-c <- a file; needs absent.txt",
                "generated: out-d <- a script",
                "failed: out-e <- a failing script; exit 3",
                "  stopped",
            ],
        )


if __name__ == "__main__":
    unittest.main()
