"""The Tauri npm packages sit on the major.minor of the crates they pair with.

`tauri build` refuses an @tauri-apps package whose major.minor differs from its
crate's, and CI never runs it, so a bump to one side alone would first fail on
the release machine.
"""

import pathlib
import re
import tomllib
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent


def crate_versions(lock_text: str) -> dict[str, list[str]]:
    versions: dict[str, list[str]] = {}
    for package in tomllib.loads(lock_text)["package"]:
        versions.setdefault(package["name"], []).append(package["version"])
    return versions


def npm_versions(lock_text: str) -> dict[str, str]:
    """Resolved versions of the root importer's @tauri-apps dependencies."""
    root = re.search(r"^importers:\n\n  \.:\n(.*?)(?=^  \S|^\S)", lock_text, re.MULTILINE | re.DOTALL)
    if root is None:
        raise AssertionError("pnpm-lock.yaml has no root importer")
    found = re.findall(
        r"^      '(@tauri-apps/[^']+)':\n        specifier: [^\n]+\n        version: ([0-9][^\s(]*)",
        root.group(1),
        re.MULTILINE,
    )
    return dict(found)


def crate_for(npm_name: str) -> str:
    """The crate an @tauri-apps package must match."""
    short = npm_name.removeprefix("@tauri-apps/")
    if short in ("api", "cli"):
        return "tauri"
    if short.startswith("plugin-"):
        return "tauri-" + short
    raise AssertionError(f"no crate pairs with {npm_name}")


def major_minor(version: str) -> tuple[int, int]:
    major, minor = version.split(".")[:2]
    return int(major), int(minor)


def mismatches(npm: dict[str, str], crates: dict[str, list[str]]) -> list[str]:
    found = []
    for name, version in sorted(npm.items()):
        crate = crate_for(name)
        for crate_version in crates.get(crate, []):
            if major_minor(crate_version) != major_minor(version):
                found.append(f"{name} {version} / {crate} {crate_version}")
    return found


class TauriPackageVersionTests(unittest.TestCase):
    def test_every_tauri_package_matches_its_crate(self) -> None:
        npm = npm_versions((REPO_ROOT / "pnpm-lock.yaml").read_text())
        crates = crate_versions((REPO_ROOT / "Cargo.lock").read_text())

        self.assertIn("@tauri-apps/api", npm)
        self.assertIn("@tauri-apps/cli", npm)
        self.assertIn("tauri", crates)
        self.assertEqual(mismatches(npm, crates), [])

    def test_a_minor_apart_is_a_mismatch(self) -> None:
        crates = {"tauri": ["2.12.3"], "tauri-plugin-dialog": ["2.8.2"]}
        self.assertEqual(
            mismatches({"@tauri-apps/api": "2.11.1", "@tauri-apps/plugin-dialog": "2.8.0"}, crates),
            ["@tauri-apps/api 2.11.1 / tauri 2.12.3"],
        )

    def test_a_package_without_its_crate_is_not_compared(self) -> None:
        self.assertEqual(mismatches({"@tauri-apps/plugin-fs": "2.1.0"}, {"tauri": ["2.12.3"]}), [])

    def test_the_root_importer_is_read_from_the_lockfile(self) -> None:
        lock = (
            "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n"
            "      '@tauri-apps/api':\n        specifier: ^2.12.3\n        version: 2.12.3\n"
            "      solid-js:\n        specifier: ^1.9.17\n        version: 1.9.17\n"
            "    devDependencies:\n"
            "      '@tauri-apps/cli':\n        specifier: ^2.12.2\n        version: 2.12.2\n"
            "\npackages:\n"
        )
        self.assertEqual(
            npm_versions(lock), {"@tauri-apps/api": "2.12.3", "@tauri-apps/cli": "2.12.2"}
        )


if __name__ == "__main__":
    unittest.main()
