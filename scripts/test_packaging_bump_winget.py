import importlib.util
import pathlib
import unittest


SCRIPT_PATH = pathlib.Path(__file__).with_name("packaging_bump_winget.py")
SPEC = importlib.util.spec_from_file_location("packaging_bump_winget", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Could not load {SCRIPT_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


STALE_LOCALE = """ShortDescription: Lightweight text editor.
Description: |-
  Old scratchpad copy.
Moniker: writ
Tags:
  - scratchpad
  - editor
ReleaseNotesUrl: https://github.com/ibrahemid/writ/releases/tag/v0.3.5
ManifestType: defaultLocale
"""


class RewriteLocaleTests(unittest.TestCase):
    def test_replaces_every_public_description_field(self) -> None:
        release_notes_url = "https://github.com/ibrahemid/writ/releases/tag/v1.0.0"

        result = MODULE.rewrite_locale(STALE_LOCALE, release_notes_url)

        self.assertIn(f"ShortDescription: {MODULE.SHORT_DESCRIPTION}", result)
        self.assertIn(f"  {MODULE.DESCRIPTION}\n", result)
        for tag in MODULE.TAGS:
            self.assertIn(f"  - {tag}\n", result)
        self.assertIn(f"ReleaseNotesUrl: {release_notes_url}", result)
        self.assertNotIn("scratchpad", result)
        self.assertNotIn("Lightweight text editor", result)

    def test_refuses_a_locale_without_all_required_fields(self) -> None:
        with self.assertRaises(MODULE.ManifestFormatError):
            MODULE.rewrite_locale("ShortDescription: incomplete\n", "https://example.com")


if __name__ == "__main__":
    unittest.main()
