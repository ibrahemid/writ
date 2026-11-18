import json
import pathlib
import re
import unittest


HOMEBREW_DIR = pathlib.Path(__file__).resolve().parent.parent / "packaging/homebrew"
CASK_PATH = HOMEBREW_DIR / "Casks/jotra.rb"
RENAMES_PATH = HOMEBREW_DIR / "cask_renames.json"


def read_stanza(cask: str, name: str) -> str:
    match = re.search(rf"^  {name} .*?(?=^\n|^  [a-z_]+ |^end$)", cask, flags=re.MULTILINE | re.DOTALL)
    if match is None:
        raise AssertionError(f"no {name} stanza in {CASK_PATH}")
    return match.group(0)


def read_quoted(stanza: str) -> list[str]:
    return re.findall(r'"([^"]+)"', stanza)


class LegacyCaskTests(unittest.TestCase):
    def test_uninstall_deletes_the_bundle_under_both_names(self) -> None:
        stanza = read_stanza(CASK_PATH.read_text(), "uninstall")

        self.assertRegex(stanza, r'\bpkgutil:\s+"com\.writ\.editor"')
        self.assertRegex(stanza, r'\bquit:\s+"com\.writ\.editor"')
        self.assertIn("delete:", stanza)
        delete = read_quoted(stanza[stanza.index("delete:") :])
        self.assertEqual(delete, ["/Applications/Jotra.app", "/Applications/Writ.app"])

    def test_zap_covers_the_support_cache_and_log_folders_under_both_names(self) -> None:
        trash = read_quoted(read_stanza(CASK_PATH.read_text(), "zap"))

        for folder in ("Application Support", "Caches", "Logs"):
            for name in ("jotra", "writ"):
                with self.subTest(folder=folder, name=name):
                    self.assertIn(f"~/Library/{folder}/{name}", trash)
        self.assertIn("~/Library/Preferences/com.writ.editor.plist", trash)

    def test_the_tap_renames_the_old_cask_to_the_new_one(self) -> None:
        self.assertEqual(json.loads(RENAMES_PATH.read_text()), {"writ": "jotra"})


if __name__ == "__main__":
    unittest.main()
