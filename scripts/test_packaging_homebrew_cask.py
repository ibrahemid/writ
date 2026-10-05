import pathlib
import re
import unittest


CASK_PATH = pathlib.Path(__file__).resolve().parent.parent / "packaging/homebrew/Casks/writ.rb"

DEPRECATED_URL_PARAMETERS = ("verified",)


def url_stanza(cask: str) -> str:
    match = re.search(r'^  url "[^"]+"(?:,\n(?:      .*\n?)+)?', cask, flags=re.MULTILINE)
    if match is None:
        raise AssertionError(f"no url stanza in {CASK_PATH}")
    return match.group(0)


class CaskUrlTests(unittest.TestCase):
    def test_url_stanza_carries_no_deprecated_parameter(self) -> None:
        stanza = url_stanza(CASK_PATH.read_text())

        for parameter in DEPRECATED_URL_PARAMETERS:
            with self.subTest(parameter=parameter):
                self.assertNotRegex(stanza, rf"\b{parameter}:")

    def test_url_points_at_the_universal_pkg_of_the_cask_version(self) -> None:
        stanza = url_stanza(CASK_PATH.read_text())

        self.assertIn(
            'url "https://github.com/ibrahemid/writ/releases/download/v#{version}/Writ_#{version}_universal.pkg"',
            stanza,
        )


if __name__ == "__main__":
    unittest.main()
