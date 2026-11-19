"""Tests for windows_signing.py.

Run from the repository root:
    python3 -m unittest discover -s .github/scripts -t .github/scripts
"""

from __future__ import annotations

import contextlib
import io
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest
import zipfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import windows_signing  # noqa: E402

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
TAURI_SCHEMA = REPO_ROOT / "node_modules" / "@tauri-apps" / "cli" / "config.schema.json"
SIGNTOOL = r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe"
ACCOUNT = {"endpoint": "https://eus.codesigning.azure.net", "account": "writ", "profile": "writ-public"}


def write_package(path: pathlib.Path, entries: dict[str, bytes]) -> None:
    with zipfile.ZipFile(path, "w") as archive:
        for name, data in entries.items():
            archive.writestr(name, data)


CLIENT_ENTRIES = {
    "bin/x64/Azure.CodeSigning.Dlib.dll": b"dlib",
    "bin/x64/Azure.CodeSigning.Dlib.Core.dll": b"core",
    "bin/x86/Azure.CodeSigning.Dlib.dll": b"x86",
    "Microsoft.ArtifactSigning.Client.nuspec": b"<package/>",
}


class WindowsSigningTests(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="jotra-windows-signing-test-")
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.package = self.root / "client.nupkg"
        write_package(self.package, CLIENT_ENTRIES)
        self.out = self.root / "windows-signing"

    def write(self, **overrides: str) -> pathlib.Path:
        fields = {**ACCOUNT, **overrides}
        return windows_signing.write_config(
            self.package, SIGNTOOL, fields["endpoint"], fields["account"], fields["profile"], self.out
        )

    def test_the_overlay_signs_through_sign_command_with_the_dlib(self) -> None:
        overlay = json.loads(self.write().read_text())
        bundle = overlay["bundle"]
        self.assertIs(bundle["createUpdaterArtifacts"], False)
        command = bundle["windows"]["signCommand"]
        self.assertEqual(command["cmd"], SIGNTOOL)
        args = command["args"]
        self.assertEqual(args[0], "sign")
        self.assertEqual(args[-1], "%1")
        self.assertEqual(args[args.index("/fd") + 1], "SHA256")
        self.assertEqual(args[args.index("/td") + 1], "SHA256")
        self.assertEqual(args[args.index("/tr") + 1], "http://timestamp.acs.microsoft.com")
        dlib = pathlib.Path(args[args.index("/dlib") + 1])
        self.assertTrue(dlib.is_absolute())
        self.assertEqual(dlib.read_bytes(), b"dlib")
        self.assertIn("x64", dlib.parts)
        metadata = pathlib.Path(args[args.index("/dmdf") + 1])
        self.assertTrue(metadata.is_absolute())
        self.assertEqual(
            json.loads(metadata.read_text()),
            {
                "Endpoint": "https://eus.codesigning.azure.net",
                "CodeSigningAccountName": "writ",
                "CertificateProfileName": "writ-public",
            },
        )

    def test_only_the_x64_client_is_unpacked(self) -> None:
        self.write()
        unpacked = sorted(str(path.relative_to(self.out / "client")) for path in (self.out / "client").rglob("*") if path.is_file())
        self.assertEqual(
            unpacked,
            [str(pathlib.PurePath("bin", "x64", name)) for name in ("Azure.CodeSigning.Dlib.Core.dll", "Azure.CodeSigning.Dlib.dll")],
        )

    def test_an_empty_or_insecure_account_field_is_refused(self) -> None:
        for overrides in ({"endpoint": ""}, {"account": " "}, {"profile": ""}, {"endpoint": "http://eus.codesigning.azure.net"}):
            with self.subTest(overrides=overrides):
                with self.assertRaises(windows_signing.WindowsSigningError):
                    self.write(**overrides)

    def test_a_package_without_the_dlib_is_refused(self) -> None:
        write_package(self.package, {"bin/x86/Azure.CodeSigning.Dlib.dll": b"x86"})
        with self.assertRaisesRegex(windows_signing.WindowsSigningError, "has no bin/x64/Azure.CodeSigning.Dlib.dll"):
            self.write()

    def test_an_entry_that_escapes_the_folder_is_refused(self) -> None:
        write_package(self.package, {**CLIENT_ENTRIES, "bin/x64/../../../escaped.dll": b"x"})
        with self.assertRaisesRegex(windows_signing.WindowsSigningError, "outside its folder"):
            self.write()
        self.assertFalse((self.root / "escaped.dll").exists())

    def test_a_package_that_is_not_a_zip_is_refused(self) -> None:
        self.package.write_bytes(b"not a zip")
        with self.assertRaisesRegex(windows_signing.WindowsSigningError, "cannot unpack"):
            self.write()

    def test_the_command_line_prints_the_overlay_path(self) -> None:
        stdout = io.StringIO()
        with contextlib.redirect_stdout(stdout):
            code = windows_signing.main(
                [
                    "--client-package", str(self.package),
                    "--signtool", SIGNTOOL,
                    "--endpoint", ACCOUNT["endpoint"],
                    "--account", ACCOUNT["account"],
                    "--profile", ACCOUNT["profile"],
                    "--out-dir", str(self.out),
                ]
            )
        self.assertEqual(code, 0)
        self.assertTrue(pathlib.Path(stdout.getvalue().strip()).is_file())
        with contextlib.redirect_stderr(io.StringIO()) as stderr:
            code = windows_signing.main(
                [
                    "--client-package", str(self.package),
                    "--signtool", SIGNTOOL,
                    "--endpoint", "",
                    "--account", "a",
                    "--profile", "p",
                    "--out-dir", str(self.out),
                ]
            )
        self.assertEqual(code, 1)
        self.assertIn("endpoint", stderr.getvalue())

    def test_one_file_is_signed_with_the_overlays_command(self) -> None:
        config = self.write()
        target = self.root / "launcher.exe"
        target.write_bytes(b"MZ")
        ran: list[list[str]] = []

        def run(command: list[str]) -> subprocess.CompletedProcess:
            ran.append(list(command))
            return subprocess.CompletedProcess(command, 0)

        windows_signing.sign_file(config, target, run)

        args = json.loads(config.read_text())["bundle"]["windows"]["signCommand"]["args"]
        self.assertEqual(ran, [[SIGNTOOL, *args[:-1], str(target.resolve())]])

    def test_a_failed_signature_is_an_error(self) -> None:
        config = self.write()
        target = self.root / "launcher.exe"
        target.write_bytes(b"MZ")
        with self.assertRaisesRegex(windows_signing.WindowsSigningError, r"signtool\.exe exited with 3 while signing"):
            windows_signing.sign_file(config, target, lambda command: subprocess.CompletedProcess(command, 3))

    def test_a_missing_file_or_an_unusable_config_signs_nothing(self) -> None:
        config = self.write()
        target = self.root / "launcher.exe"
        target.write_bytes(b"MZ")
        overlay = json.loads(config.read_text())
        without_file = self.root / "without-file.json"
        overlay["bundle"]["windows"]["signCommand"]["args"].remove("%1")
        without_file.write_text(json.dumps(overlay))
        not_json = self.root / "not-json.json"
        not_json.write_text("{")
        cases = (
            (config, self.root / "absent.exe", "no file to sign"),
            (without_file, target, "no %1 argument"),
            (not_json, target, "holds no bundle.windows.signCommand"),
            (self.root / "absent.json", target, "holds no bundle.windows.signCommand"),
        )
        for config_path, file, message in cases:
            with self.subTest(message=message, config=config_path.name):
                with self.assertRaisesRegex(windows_signing.WindowsSigningError, message):
                    windows_signing.sign_file(config_path, file, lambda command: self.fail(f"ran {command}"))

    def test_the_sign_subcommand_reports_a_refusal(self) -> None:
        config = self.write()
        with contextlib.redirect_stderr(io.StringIO()) as stderr:
            code = windows_signing.main(["sign", str(config), str(self.root / "absent.exe")])
        self.assertEqual(code, 1)
        self.assertIn("no file to sign", stderr.getvalue())

    @unittest.skipUnless(TAURI_SCHEMA.exists(), "the Tauri CLI is not installed (pnpm install)")
    def test_the_sign_command_matches_the_tauri_config_schema(self) -> None:
        schema = json.loads(TAURI_SCHEMA.read_text())
        definitions = schema.get("definitions") or schema["$defs"]
        object_form = next(
            option for option in definitions["CustomSignCommandConfig"]["anyOf"] if option.get("type") == "object"
        )
        command = json.loads(self.write().read_text())["bundle"]["windows"]["signCommand"]
        self.assertEqual(set(command), set(object_form["properties"]))
        self.assertTrue(set(object_form["required"]) <= set(command))
        self.assertIsInstance(command["cmd"], str)
        self.assertTrue(all(isinstance(arg, str) for arg in command["args"]))
        windows = definitions["WindowsConfig"]["properties"]
        self.assertIn("signCommand", windows)
        self.assertIn("createUpdaterArtifacts", definitions["BundleConfig"]["properties"])


if __name__ == "__main__":
    unittest.main()
