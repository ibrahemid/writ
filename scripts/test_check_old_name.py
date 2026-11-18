import contextlib
import importlib.util
import io
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest


SCRIPT_PATH = pathlib.Path(__file__).with_name("check_old_name.py")
SPEC = importlib.util.spec_from_file_location("check_old_name", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Could not load {SCRIPT_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)

OLD = MODULE.get_name_forms("Writ")


def make_scanner(stage="app", allowlist="", files=()):
    return MODULE.NameScanner(OLD, stage, MODULE.parse_allowlist(allowlist), list(files))


def get_matched(text, stage="app", allowlist="", path="notes.txt", files=()):
    scanner = make_scanner(stage, allowlist, files)
    return [match.group() for match in scanner.find_text_matches(text, path)]


def write_tree(root, files):
    for relative, content in files.items():
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, bytes):
            path.write_bytes(content)
        else:
            path.write_text(content, encoding="utf-8")


class NamePatternTests(unittest.TestCase):
    def test_words_that_contain_the_letters_are_not_matches(self) -> None:
        for word in ("write", "rewrite", "writable", "WRITE", "typewriter", "underwrite", "written", "Writer", "OverWrite"):
            with self.subTest(word=word):
                self.assertEqual(get_matched(word), [])

    def test_every_spelling_of_the_name_matches(self) -> None:
        cases = {
            "Writ": "Writ",
            "writ-core": "writ",
            "writ_core": "writ",
            "WRIT_DATA_DIR": "WRIT",
            "isWritCommand": "Writ",
            "--writ-accent": "writ",
            "writ://open": "writ",
            "VITE_WRIT_DEMO": "WRIT",
            "WritOnDevice": "Writ",
            "writ2": "writ",
        }
        for text, expected in cases.items():
            with self.subTest(text=text):
                self.assertEqual(get_matched(text), [expected])

    def test_escapes_and_a_lib_prefix_end_the_preceding_word(self) -> None:
        cases = {
            "libwrit_on_device.a": "writ",
            '"text\\nwrit-proposal"': "writ",
            "\\rwrit": "writ",
            "\\tWRIT_DIR": "WRIT",
            "%2FWrit.app": "Writ",
            "%2Fwrit%22": "writ",
            "%5CWrit%5C": "Writ",
            "..%2fwrit.db": "writ",
        }
        for text, expected in cases.items():
            with self.subTest(text=text):
                self.assertEqual(get_matched(text), [expected])

    def test_escape_and_lib_boundaries_keep_the_word_rules(self) -> None:
        for text in ("glibwrit", "calibwrit", "libwrite", "\\nwrite", "%2Fwritten", "\\nWRITE", "%ZZwrit", "\\bwrit"):
            with self.subTest(text=text):
                self.assertEqual(get_matched(text), [])

    def test_the_pattern_follows_the_old_name_given(self) -> None:
        scanner = MODULE.NameScanner(MODULE.get_name_forms("Quoll"), "app", MODULE.parse_allowlist(""), [])
        found = [match.group() for match in scanner.find_text_matches("quoll-core isQuollCommand QUOLL_DIR Writ", "a.txt")]
        self.assertEqual(found, ["quoll", "Quoll", "QUOLL"])

    def test_a_name_must_be_capitalised_letters_or_digits(self) -> None:
        for name in ("writ", "WRIT", "WrIt", "Wr-t", ""):
            with self.subTest(name=name):
                with self.assertRaises(MODULE.InvalidNameError):
                    MODULE.get_name_forms(name)


class KeptTokenTests(unittest.TestCase):
    def test_kept_tokens_are_masked_in_both_stages(self) -> None:
        kept = (
            "com.writ.editor",
            "com.writ.editor/writ-command",
            "com.writ.ai",
            "com.writ.rust-source",
            'id.starts_with("com.writ.")',
            'Some("COM.WRIT.EDITOR")',
            "Accent::WritBlue,",
            "writ-light",
            "writ-dark",
            "writ-blue",
            "ibrahemid.Writ",
            "packaging/aur/writ-bin/PKGBUILD",
            "ibrahemid/homebrew-writ",
            "brew tap ibrahemid/writ",
            "brew untap ibrahemid/writ",
            "brew trust ibrahemid/writ",
            "ab97cc4d-eea3-53ee-90d8-3c2767967676",
        )
        for stage in MODULE.STAGES:
            for text in kept:
                with self.subTest(stage=stage, text=text):
                    self.assertEqual(get_matched(text, stage), [])

    def test_a_masked_token_does_not_hide_its_neighbours(self) -> None:
        self.assertEqual(get_matched("com.writ.editor writ-core"), ["writ"])
        self.assertEqual(get_matched("writ-darkroom"), ["writ"])
        self.assertEqual(get_matched("WritBlueprint COM.WRIT_DIR"), ["Writ", "WRIT"])

    def test_the_cask_in_a_tap_path_is_renamed_but_not_the_tap(self) -> None:
        text = "brew install --cask ibrahemid/writ/writ"
        for stage in MODULE.STAGES:
            with self.subTest(stage=stage):
                matches = make_scanner(stage).find_text_matches(text, "README.md")
                self.assertEqual([match.start() for match in matches], [len(text) - 4])


class StageTests(unittest.TestCase):
    def test_app_stage_keeps_the_slug_and_host(self) -> None:
        text = 'url = "https://github.com/ibrahemid/writ/releases"\nsite = "https://writ.ibrahemid.com/docs"\nREPO="ibrahemid/writ"\n'
        self.assertEqual(get_matched(text, "app"), [])

    def test_channels_stage_reports_the_slug_and_host(self) -> None:
        text = 'url = "https://github.com/ibrahemid/writ/releases"\nsite = "https://writ.ibrahemid.com/docs"\nREPO="ibrahemid/writ"\n'
        self.assertEqual(get_matched(text, "channels"), ["writ", "writ", "writ"])

    def test_a_slug_joined_as_path_parts_follows_the_slug(self) -> None:
        text = 'server = root / "ibrahemid" / "writ.git"\nother = root / \'ibrahemid\' / \'writ\'\n'
        self.assertEqual(get_matched(text, "app"), [])
        self.assertEqual(get_matched(text, "channels"), ["writ", "writ"])
        self.assertEqual(get_matched('root / "ibrahemid" / "notes" / "writ.md"', "app"), ["writ"])

    def test_an_unknown_stage_is_refused(self) -> None:
        with self.assertRaises(MODULE.OldNameError):
            make_scanner("release")


class GlobTests(unittest.TestCase):
    def assertGlob(self, glob, matching, other):
        pattern = MODULE.compile_glob(glob)
        for path in matching:
            self.assertIsNotNone(pattern.fullmatch(path), f"{glob} should match {path}")
        for path in other:
            self.assertIsNone(pattern.fullmatch(path), f"{glob} should not match {path}")

    def test_character_classes_bound_the_decision_records(self) -> None:
        self.assertGlob(
            "docs/adr/0[0-5][0-9]-*.md",
            ["docs/adr/041-writ-is-a-text-app.md", "docs/adr/059-rename.md"],
            ["docs/adr/060-later.md", "docs/adr/sub/041-x.md", "docs/adr/041-x.txt"],
        )

    def test_double_star_spans_directories(self) -> None:
        self.assertGlob("**/Writ.desktop", ["Writ.desktop", "src-tauri/linux/Writ.desktop"], ["Writ.desktop.bak"])
        self.assertGlob("packaging/aur/writ-bin/**", ["packaging/aur/writ-bin/PKGBUILD", "packaging/aur/writ-bin/a/b"], ["packaging/aur/PKGBUILD"])
        self.assertGlob("**", ["a", "a/b/c"], [])

    def test_single_star_stays_in_one_segment(self) -> None:
        self.assertGlob(
            "crates/*-core/src/legacy_names.rs",
            ["crates/writ-core/src/legacy_names.rs", "crates/other-core/src/legacy_names.rs"],
            ["crates/a/b-core/src/legacy_names.rs"],
        )

    def test_negated_classes_and_question_marks(self) -> None:
        self.assertGlob("a/[!x]?.txt", ["a/yz.txt"], ["a/xz.txt", "a/y.txt"])

    def test_an_unclosed_class_is_a_format_error(self) -> None:
        with self.assertRaises(MODULE.AllowlistFormatError):
            MODULE.compile_glob("docs/[0-9.md")


class AllowlistTests(unittest.TestCase):
    def test_parses_scopes_and_reasons(self) -> None:
        allowlist = MODULE.parse_allowlist(
            "# comment\n\n"
            "docs/** -- history\n"
            "CHANGELOG.md :from /^## \\[[0-9]/ -- released sections\n"
            "install.sh :lines /(?i)legacy/ -- legacy branch\n"
            "** :text /writ-tauri-signing/ -- key name\n"
            "themes/writ-*.json :path -- kept ids\n"
        )
        self.assertEqual(
            [(rule.glob, rule.scope, rule.reason) for rule in allowlist.rules],
            [
                ("docs/**", None, "history"),
                ("CHANGELOG.md", "from", "released sections"),
                ("install.sh", "lines", "legacy branch"),
                ("**", "text", "key name"),
                ("themes/writ-*.json", "path", "kept ids"),
            ],
        )

    def test_a_rule_without_a_reason_is_refused(self) -> None:
        for line in ("docs/**", "docs/** --", "docs/** :lines /x/", "docs/** :other /x/ -- why"):
            with self.subTest(line=line):
                with self.assertRaises(MODULE.AllowlistFormatError):
                    MODULE.parse_allowlist(line)

    def test_a_broken_pattern_is_refused(self) -> None:
        with self.assertRaises(MODULE.AllowlistFormatError):
            MODULE.parse_allowlist("a.txt :lines /(unclosed/ -- why")

    def test_from_scope_keeps_the_rest_of_the_file(self) -> None:
        text = "# Changelog\n## Unreleased\n- Writ\n## [1.0.1] - 2026-10-27\n- Writ\n- writ\n"
        allowlist = "CHANGELOG.md :from /^## \\[[0-9]/ -- released"
        self.assertEqual(get_matched(text, allowlist=allowlist, path="CHANGELOG.md"), ["Writ"])

    def test_lines_scope_keeps_matching_lines(self) -> None:
        text = 'LEGACY_DIR="$HOME/.writ"\nNEW_DIR="$HOME/.writ"\n'
        allowlist = "install.sh :lines /(?i)legacy/ -- legacy branch"
        self.assertEqual(get_matched(text, allowlist=allowlist, path="install.sh"), ["writ"])

    def test_text_scope_keeps_matching_text(self) -> None:
        text = "key ~/.tauri/writ.key and Writ\n"
        allowlist = "** :text /\\.tauri\\/writ\\.key/ -- key name"
        self.assertEqual(get_matched(text, allowlist=allowlist, path="docs/RELEASING.md"), ["Writ"])

    def test_whole_file_rules_keep_text_and_path(self) -> None:
        allowlist = "docs/adr/0[0-5][0-9]-*.md -- history"
        path = "docs/adr/041-writ-is-a-text-app.md"
        scanner = make_scanner(allowlist=allowlist, files=[path])
        self.assertEqual(scanner.find_text_matches("Writ", path), [])
        self.assertEqual(scanner.find_path_matches(path), [])

    def test_path_rules_keep_the_name_and_scan_the_text(self) -> None:
        allowlist = "themes/writ-*.json :path -- kept ids"
        path = "themes/writ-classic.json"
        scanner = make_scanner(allowlist=allowlist, files=[path])
        self.assertEqual(scanner.find_path_matches(path), [])
        self.assertEqual([m.group() for m in scanner.find_text_matches('{"name": "Writ Classic"}', path)], ["Writ"])

    def test_scoped_rules_leave_the_path_to_the_scan(self) -> None:
        allowlist = "Casks/*.rb :from /^\\s*uninstall\\b/ -- uninstall paths"
        scanner = make_scanner(allowlist=allowlist, files=["Casks/writ.rb"])
        self.assertEqual([m.group() for m in scanner.find_path_matches("Casks/writ.rb")], ["writ"])

    def test_literal_directories_in_a_rule_are_kept(self) -> None:
        allowlist = "manifests/ibrahemid/Writ/** -- history"
        path = "manifests/ibrahemid/Writ/1.0.0/ibrahemid.Writ.yaml"
        self.assertEqual(make_scanner(allowlist=allowlist, files=[path]).find_path_matches(path), [])

    def test_wildcard_directories_in_a_rule_are_renamed(self) -> None:
        allowlist = "crates/*-core/src/legacy_names.rs -- legacy"
        path = "crates/writ-core/src/legacy_names.rs"
        scanner = make_scanner(allowlist=allowlist, files=[path])
        self.assertEqual([m.start() for m in scanner.find_path_matches(path)], [len("crates/")])
        self.assertEqual(scanner.find_text_matches("use writ_core::x;", path), [])

    def test_references_to_a_kept_file_are_masked(self) -> None:
        allowlist = "docs/adr/0[0-5][0-9]-*.md -- history"
        files = ["docs/adr/041-writ-is-a-text-app.md", "README.md"]
        text = "See docs/adr/041-writ-is-a-text-app.md and [the record](adr/041-writ-is-a-text-app) for Writ.\n"
        self.assertEqual(get_matched(text, allowlist=allowlist, path="README.md", files=files), ["Writ"])


class GuardTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name) / "tree"
        self.allowlist = pathlib.Path(self.tmp.name) / "allowlist.txt"
        self.allowlist.write_text("docs/adr/0[0-5][0-9]-*.md -- history\n", encoding="utf-8")

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def run_guard(self, stage="app"):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            code = MODULE.main(["--stage", stage, "--root", str(self.root), "--allowlist", str(self.allowlist)])
        return code, output.getvalue().splitlines()

    def test_reports_each_hit_with_its_position_and_a_count(self) -> None:
        write_tree(
            self.root,
            {
                "docs/adr/041-writ-is-a-text-app.md": "Writ\n",
                "crates/writ-core/src/lib.rs": "// ok\npub fn emit() { send(\"writ://open\"); }\n",
                "src/a.ts": "const isWritCommand = true;\n",
                "assets/logo.bin": b"\x00Writ\x00",
                "node_modules/pkg/index.js": "writ",
            },
        )
        code, lines = self.run_guard()
        self.assertEqual(code, 1)
        self.assertEqual(
            lines,
            [
                "crates/writ-core/src/lib.rs:0:8 writ-core",
                "crates/writ-core/src/lib.rs:2:23 writ://",
                "src/a.ts:1:9 isWritCommand",
                "3 hits of Writ outside the allowlist",
            ],
        )

    def test_a_clean_tree_passes(self) -> None:
        write_tree(self.root, {"src/a.ts": "const rewrite = 'write';\n", "docs/adr/041-writ-is-a-text-app.md": "Writ\n"})
        code, lines = self.run_guard()
        self.assertEqual(code, 0)
        self.assertEqual(lines, ["0 hits of Writ outside the allowlist"])

    def test_the_stage_decides_the_slug(self) -> None:
        write_tree(self.root, {"install.sh": 'REPO="ibrahemid/writ"\n'})
        self.assertEqual(self.run_guard("app")[0], 0)
        code, lines = self.run_guard("channels")
        self.assertEqual(code, 1)
        self.assertEqual(lines[0], "install.sh:1:17 writ")

    def test_a_missing_allowlist_is_an_error(self) -> None:
        write_tree(self.root, {"a.txt": "x"})
        self.allowlist.unlink()
        with contextlib.redirect_stderr(io.StringIO()) as errors:
            code, _ = self.run_guard()
        self.assertEqual(code, 2)
        self.assertIn("allowlist", errors.getvalue())

    @unittest.skipUnless(shutil.which("git"), "git is not installed")
    def test_a_git_tree_lists_tracked_and_untracked_files_but_not_ignored_ones(self) -> None:
        write_tree(
            self.root,
            {".gitignore": "ignored/\n", "tracked.txt": "Writ\n", "untracked.txt": "Writ\n", "ignored/x.txt": "Writ\n"},
        )
        subprocess.run(["git", "init", "-q"], cwd=self.root, check=True)
        subprocess.run(["git", "add", "tracked.txt", ".gitignore"], cwd=self.root, check=True)
        self.assertEqual(MODULE.list_files(self.root), [".gitignore", "tracked.txt", "untracked.txt"])


class RepositoryAllowlistTests(unittest.TestCase):
    def test_the_repository_allowlist_parses(self) -> None:
        allowlist = MODULE.load_allowlist(SCRIPT_PATH.parent / "old-name-allowlist.txt")
        self.assertTrue(allowlist.rules)
        self.assertTrue(all(rule.reason for rule in allowlist.rules))

    def test_the_rename_decision_record_is_kept_whole(self) -> None:
        allowlist = MODULE.load_allowlist(SCRIPT_PATH.parent / "old-name-allowlist.txt")
        records = sorted((SCRIPT_PATH.parent.parent / "docs" / "adr").glob("059-rename-to-*.md"))
        self.assertTrue(records)
        for record in records:
            path = f"docs/adr/{record.name}"
            with self.subTest(path=path):
                self.assertTrue(allowlist.is_whole_file(path))
        self.assertTrue(allowlist.is_whole_file("docs/adr/059-rename-to-jotra.md"))
        self.assertFalse(allowlist.is_whole_file("docs/adr/060-rename-to-jotra.md"))

    def test_the_maintainer_key_name_is_kept_in_every_spelling(self) -> None:
        allowlist = (SCRIPT_PATH.parent / "old-name-allowlist.txt").read_text(encoding="utf-8")
        cases = (
            'UPDATER_KEY_PATH="${HOME}/.tauri/writ.key"',
            'key_file = self.home / ".tauri" / "writ.key"',
            "f\"no key at {self.home / '.tauri' / 'writ.key'}\"",
            "security find-generic-password -s writ-tauri-signing -w",
        )
        for text in cases:
            with self.subTest(text=text):
                self.assertEqual(get_matched(text, allowlist=allowlist, path="scripts/release-preflight.sh"), [])
        self.assertEqual(get_matched('self.root / "writ.key"', allowlist=allowlist, path="a.py"), ["writ"])

    def test_the_azure_signing_account_is_kept_in_its_fixtures(self) -> None:
        allowlist = (SCRIPT_PATH.parent / "old-name-allowlist.txt").read_text(encoding="utf-8")
        path = ".github/scripts/test_windows_signing.py"
        for text in ('"account": "writ",', '"CodeSigningAccountName": "writ",', '"profile": "writ-public",'):
            with self.subTest(text=text):
                self.assertEqual(get_matched(text, allowlist=allowlist, path=path), [])
        self.assertEqual(get_matched('prefix="writ-windows-signing-test-"', allowlist=allowlist, path=path), ["writ"])


if __name__ == "__main__":
    unittest.main()
