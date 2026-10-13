"""Tests for release_assets.py.

Run from the repository root:
    python3 -m unittest discover -s .github/scripts -t .github/scripts
"""

from __future__ import annotations

import contextlib
import io
import json
import pathlib
import re
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import build_latest_json  # noqa: E402
import release_assets  # noqa: E402

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
VERSION = "0.6.0"
SERIAL = "urn:uuid:3e671687-395b-41f5-a30f-a58921a69b79"


def sbom_document(sbom: release_assets.Sbom, version: str = VERSION) -> dict[str, object]:
    return {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": SERIAL,
        "version": 1,
        "metadata": {"component": {"type": "application", "name": sbom.component, "version": version}},
        "components": [{"type": "library", "name": "serde", "version": "1.0.228"}],
    }


def run_main(*argv: str) -> tuple[int, str, str]:
    stdout, stderr = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
        code = release_assets.main(list(argv))
    return code, stdout.getvalue(), stderr.getvalue()


class AssetNameTests(unittest.TestCase):
    def test_installers_follow_the_bundler_names(self) -> None:
        self.assertEqual(
            [i.name for i in release_assets.installers(VERSION)],
            [
                "Writ_0.6.0_universal.pkg",
                "Writ_0.6.0_universal.dmg",
                "Writ_0.6.0_universal.app.tar.gz",
                "Writ_0.6.0_x64_en-US.msi",
                "Writ_0.6.0_amd64.deb",
                "Writ_0.6.0_amd64.AppImage",
            ],
        )

    def test_a_candidate_version_keeps_its_suffix(self) -> None:
        names = release_assets.asset_set("0.6.0-1", "installers")
        self.assertIn("Writ_0.6.0-1_x64_en-US.msi", names)
        self.assertIn("Writ_0.6.0-1_universal.pkg", names)

    def test_every_updater_payload_is_one_the_manifest_builder_picks(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            for name in release_assets.asset_set(VERSION, "installers"):
                (root / name).write_bytes(b"payload")
            for name in release_assets.updater_signatures(VERSION):
                (root / name).write_text("signature")
            picked = {
                build_latest_json.match_one([r"\.app\.tar\.gz$"], root).name,
                build_latest_json.match_one([r"_en-US\.msi$", r"\.msi$"], root).name,
                build_latest_json.match_one([r"\.AppImage$"], root).name,
                build_latest_json.match_one([r"_amd64\.deb$", r"\.deb$"], root).name,
            }
        self.assertEqual(picked, set(release_assets.updater_payloads(VERSION)))

    def test_the_pkg_and_dmg_carry_no_updater_signature(self) -> None:
        signatures = release_assets.updater_signatures(VERSION)
        self.assertNotIn("Writ_0.6.0_universal.pkg.sig", signatures)
        self.assertNotIn("Writ_0.6.0_universal.dmg.sig", signatures)
        self.assertEqual(len(signatures), 4)

    def test_sboms_cover_both_binaries_on_every_target_and_the_frontend(self) -> None:
        names = release_assets.asset_set(VERSION, "sboms")
        self.assertEqual(len(names), 9)
        for crate in ("writ-tauri", "writ-cli"):
            for target in release_assets.SBOM_TARGETS:
                self.assertIn(f"{crate}_0.6.0_{target}.cdx.json", names)
        self.assertIn("writ-frontend_0.6.0.cdx.json", names)

    def test_a_target_sbom_describes_only_that_platforms_installers(self) -> None:
        by_name = {s.name: s for s in release_assets.sboms(VERSION)}
        windows = release_assets.sbom_subjects(VERSION, by_name["writ-cli_0.6.0_x86_64-pc-windows-msvc.cdx.json"])
        self.assertEqual(windows, ["Writ_0.6.0_x64_en-US.msi"])
        mac = release_assets.sbom_subjects(VERSION, by_name["writ-tauri_0.6.0_aarch64-apple-darwin.cdx.json"])
        self.assertEqual(
            mac, ["Writ_0.6.0_universal.pkg", "Writ_0.6.0_universal.dmg", "Writ_0.6.0_universal.app.tar.gz"]
        )
        frontend = release_assets.sbom_subjects(VERSION, by_name["writ-frontend_0.6.0.cdx.json"])
        self.assertEqual(frontend, [i.name for i in release_assets.installers(VERSION)])

    def test_the_checksums_cover_every_download_but_the_metadata(self) -> None:
        checksummed = set(release_assets.asset_set(VERSION, "checksummed"))
        published = set(release_assets.asset_set(VERSION, "published"))
        self.assertEqual(
            published - checksummed,
            {"SHA256SUMS.txt", "SHA256SUMS.txt.minisig", "latest.json", "release-meta.json"},
        )

    def test_no_sbom_name_matches_an_installer_pattern_the_site_reads(self) -> None:
        site_patterns = (r"universal\.dmg$", r"universal\.pkg$", r"\.msi$", r"\.deb$", r"\.AppImage$")
        for name in release_assets.asset_set(VERSION, "published"):
            matches = [p for p in site_patterns if re.search(p, name)]
            if name in release_assets.asset_set(VERSION, "installers"):
                continue
            with self.subTest(name=name):
                self.assertEqual(matches, [])

    def test_a_malformed_version_is_refused(self) -> None:
        for version in ("v0.6.0", "0.6", "0.6.0 ", "0.6.0/x", "0.6.0\tx", ""):
            with self.subTest(version=version):
                with self.assertRaises(release_assets.ReleaseAssetError):
                    release_assets.installers(version)


class CompareDirTests(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)

    def fill(self, names: list[str]) -> None:
        for name in names:
            (self.root / name).write_bytes(b"x")

    def test_the_exact_set_passes(self) -> None:
        names = release_assets.asset_set(VERSION, "published")
        self.fill(names)
        difference = release_assets.compare_dir(self.root, names)
        self.assertEqual((difference.missing, difference.unexpected), ([], []))

    def test_a_missing_and_an_extra_file_are_both_named(self) -> None:
        names = release_assets.asset_set(VERSION, "sboms")
        self.fill(names[1:] + ["stray.txt"])
        difference = release_assets.compare_dir(self.root, names)
        self.assertEqual(difference.missing, [names[0]])
        self.assertEqual(difference.unexpected, ["stray.txt"])

    def test_a_directory_in_place_of_a_file_is_unexpected(self) -> None:
        names = release_assets.asset_set(VERSION, "sboms")
        self.fill(names[1:])
        (self.root / names[0]).mkdir()
        difference = release_assets.compare_dir(self.root, names)
        self.assertEqual(difference.unexpected, [names[0]])

    def test_the_check_command_exits_non_zero_on_a_difference(self) -> None:
        self.fill(release_assets.asset_set(VERSION, "installers")[1:])
        code, _, stderr = run_main("check", "--dir", str(self.root), "--version", VERSION, "--set", "installers")
        self.assertEqual(code, 1)
        self.assertIn("missing: Writ_0.6.0_amd64.AppImage", stderr)

    def test_the_check_command_passes_the_exact_set(self) -> None:
        self.fill(release_assets.asset_set(VERSION, "installers"))
        code, stdout, _ = run_main("check", "--dir", str(self.root), "--version", VERSION, "--set", "installers")
        self.assertEqual(code, 0, stdout)


class SbomContentTests(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.by_name = {sbom.name: sbom for sbom in release_assets.sboms(VERSION)}
        for sbom in self.by_name.values():
            self.write(sbom.name, sbom_document(sbom))

    def write(self, name: str, document: object) -> None:
        (self.root / name).write_text(json.dumps(document))

    def check(self, set_name: str = "sboms") -> tuple[int, str]:
        code, _, stderr = run_main("check", "--dir", str(self.root), "--version", VERSION, "--set", set_name)
        return code, stderr

    def test_well_formed_sboms_pass(self) -> None:
        code, stderr = self.check()
        self.assertEqual(code, 0, stderr)

    def test_an_sbom_without_a_serial_number_is_refused(self) -> None:
        name = "writ-tauri_0.6.0_x86_64-unknown-linux-gnu.cdx.json"
        document = sbom_document(self.by_name[name])
        del document["serialNumber"]
        self.write(name, document)
        code, stderr = self.check()
        self.assertEqual(code, 1)
        self.assertIn(f"{name} has no serialNumber, which actions/attest requires of a CycloneDX SBOM", stderr)

    def test_every_field_actions_attest_requires_is_checked(self) -> None:
        name = "writ-cli_0.6.0_aarch64-apple-darwin.cdx.json"
        for field in ("bomFormat", "specVersion", "serialNumber"):
            for value in (None, ""):
                with self.subTest(field=field, value=value):
                    document = sbom_document(self.by_name[name])
                    if value is None:
                        del document[field]
                    else:
                        document[field] = value
                    self.write(name, document)
                    code, stderr = self.check()
                    self.assertEqual(code, 1)
                    self.assertIn(f"{name} has", stderr)
                    self.assertIn(field, stderr)

    def test_an_sbom_with_no_components_is_refused(self) -> None:
        name = "writ-frontend_0.6.0.cdx.json"
        for components in ([], None):
            with self.subTest(components=components):
                document = sbom_document(self.by_name[name])
                if components is None:
                    del document["components"]
                else:
                    document["components"] = components
                self.write(name, document)
                code, stderr = self.check()
                self.assertEqual(code, 1)
                self.assertIn(f"{name} lists no components", stderr)

    def test_an_sbom_of_another_project_or_version_is_refused(self) -> None:
        name = "writ-frontend_0.6.0.cdx.json"
        document = sbom_document(self.by_name[name])
        document["metadata"] = {"component": {"name": "writ-site", "version": "0.5.1"}}
        self.write(name, document)
        code, stderr = self.check()
        self.assertEqual(code, 1)
        self.assertIn(f"{name} describes 'writ-site', expected 'writ'", stderr)
        self.assertIn(f"{name} describes version '0.5.1', expected '0.6.0'", stderr)

    def test_an_sbom_naming_no_component_is_refused(self) -> None:
        name = "writ-cli_0.6.0_x86_64-pc-windows-msvc.cdx.json"
        document = sbom_document(self.by_name[name])
        del document["metadata"]
        self.write(name, document)
        code, stderr = self.check()
        self.assertEqual(code, 1)
        self.assertIn(f"{name} names no metadata.component", stderr)

    def test_a_file_that_is_not_a_json_object_is_refused(self) -> None:
        name = "writ-tauri_0.6.0_aarch64-apple-darwin.cdx.json"
        cases = ((b"{", "is not UTF-8 JSON"), (b"\xff{}", "is not UTF-8 JSON"), (b"[]", "is not a JSON object"))
        for content, message in cases:
            with self.subTest(content=content):
                (self.root / name).write_bytes(content)
                code, stderr = self.check()
                self.assertEqual(code, 1)
                self.assertIn(f"{name} {message}", stderr)

    def test_an_sbom_over_the_attest_size_limit_is_refused(self) -> None:
        name = "writ-tauri_0.6.0_x86_64-apple-darwin.cdx.json"
        with (self.root / name).open("r+b") as handle:
            handle.truncate(release_assets.ATTEST_SBOM_MAX_BYTES + 1)
        code, stderr = self.check()
        self.assertEqual(code, 1)
        self.assertIn(f"{name} is {release_assets.ATTEST_SBOM_MAX_BYTES + 1} bytes", stderr)

    def test_signing_and_publishing_read_the_sboms_too(self) -> None:
        name = "writ-cli_0.6.0_x86_64-unknown-linux-gnu.cdx.json"
        for set_name in ("signing-inputs", "published"):
            with self.subTest(set=set_name), tempfile.TemporaryDirectory() as tmp:
                root = pathlib.Path(tmp)
                for asset in release_assets.asset_set(VERSION, set_name):
                    if asset in self.by_name:
                        (root / asset).write_text(json.dumps(sbom_document(self.by_name[asset])))
                    else:
                        (root / asset).write_bytes(b"x")
                argv = ("check", "--dir", tmp, "--version", VERSION, "--set", set_name)
                self.assertEqual(run_main(*argv)[0], 0)
                document = sbom_document(self.by_name[name])
                document["components"] = []
                (root / name).write_text(json.dumps(document))
                code, _, stderr = run_main(*argv)
                self.assertEqual(code, 1)
                self.assertIn(f"{name} lists no components", stderr)

    def test_the_expected_components_are_the_ones_the_repository_builds(self) -> None:
        package = json.loads((REPO_ROOT / "package.json").read_text())
        self.assertEqual(package["name"], release_assets.FRONTEND_PACKAGE_NAME)
        manifests = {
            "writ-tauri": REPO_ROOT / "src-tauri/Cargo.toml",
            "writ-cli": REPO_ROOT / "crates/writ-cli/Cargo.toml",
        }
        self.assertEqual(set(manifests), set(release_assets.SBOM_CRATES))
        for crate, manifest in manifests.items():
            with self.subTest(crate=crate):
                self.assertRegex(manifest.read_text(), rf'(?m)^\[package\]\nname = "{re.escape(crate)}"$')


class CommandLineTests(unittest.TestCase):
    def test_sbom_matrix_lists_each_sbom_with_its_subjects(self) -> None:
        code, stdout, _ = run_main("sbom-matrix", "--version", VERSION)
        self.assertEqual(code, 0)
        matrix = json.loads(stdout)
        self.assertEqual(len(matrix), 9)
        entry = next(e for e in matrix if e["sbom"] == "writ-cli_0.6.0_x86_64-unknown-linux-gnu.cdx.json")
        self.assertEqual(entry["subjects"], "Writ_0.6.0_amd64.deb Writ_0.6.0_amd64.AppImage")

    def test_sbom_targets_prints_one_target_per_line(self) -> None:
        code, stdout, _ = run_main("sbom-targets")
        self.assertEqual(code, 0)
        self.assertEqual(stdout.split(), list(release_assets.SBOM_TARGETS))

    def test_list_prints_the_updater_payloads(self) -> None:
        code, stdout, _ = run_main("list", "--version", VERSION, "--set", "updater-payloads")
        self.assertEqual(code, 0)
        self.assertEqual(stdout.split(), sorted(release_assets.updater_payloads(VERSION)))

    def test_a_bad_version_exits_non_zero(self) -> None:
        code, _, stderr = run_main("list", "--version", "v0.6.0", "--set", "published")
        self.assertEqual(code, 1)
        self.assertIn("v0.6.0", stderr)


if __name__ == "__main__":
    unittest.main()
