"""Tests for release_signing.py, including the updater key transition rehearsal.

The tests that sign run the minisign binary named by $MINISIGN, or the one on
PATH, and skip without it. CI installs the pinned release before running them.

Run from the repository root:
    python3 -m unittest discover -s .github/scripts -t .github/scripts
"""

from __future__ import annotations

import base64
import contextlib
import io
import json
import os
import pathlib
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import build_latest_json  # noqa: E402
import release_assets  # noqa: E402
import release_signing  # noqa: E402

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
VERSION = "0.6.0"
TAG = "v0.6.0"
REPO = "ibrahemid/writ"
MINISIGN = os.environ.get("MINISIGN") or shutil.which("minisign")
TAURI_CLI = REPO_ROOT / "node_modules" / ".bin" / "tauri"

requires_minisign = unittest.skipUnless(MINISIGN, "minisign is not installed")


@contextlib.contextmanager
def scratch() -> pathlib.Path:
    with tempfile.TemporaryDirectory(prefix="writ-signing-test-") as tmp:
        yield pathlib.Path(tmp)


class UpdaterKey:
    """A throwaway key in the form `tauri signer generate` writes.

    Without a password the key is stored unencrypted, which skips minisign's
    scrypt derivation (seconds and a gigabyte per use); the tests that exercise
    the password give one.
    """

    def __init__(self, directory: pathlib.Path, label: str, password: str | None = None) -> None:
        self.password = password or ""
        secret = directory / f"{label}.key"
        public = directory / f"{label}.pub"
        command = [MINISIGN, "-G", "-f", "-p", str(public), "-s", str(secret)]
        if password is None:
            command.append("-W")
        subprocess.run(
            command,
            input=f"{self.password}\n{self.password}\n",
            capture_output=True,
            text=True,
            check=True,
        )
        self.public_text = public.read_text()
        self.private_env = base64.b64encode(secret.read_bytes()).decode("ascii")
        self.key_id = release_signing.public_key_from_text(self.public_text).key_id
        self.config = directory / f"{label}.tauri.conf.json"
        self.config.write_text(
            json.dumps(
                {"plugins": {"updater": {"pubkey": base64.b64encode(self.public_text.encode()).decode("ascii")}}}
            )
        )

    def env(self) -> dict[str, str]:
        return {
            release_signing.KEY_ENV: self.private_env,
            release_signing.PASSWORD_ENV: self.password,
        }


def write_release_inputs(directory: pathlib.Path, version: str = VERSION) -> None:
    for name in release_assets.asset_set(version, "installers"):
        (directory / name).write_bytes(f"bytes of {name}".encode())
    for name in release_assets.asset_set(version, "sboms"):
        (directory / name).write_text(json.dumps({"bomFormat": "CycloneDX", "name": name}))
    (directory / release_assets.RELEASE_META).write_text('{"schema":1}\n')


def sign_release(directory: pathlib.Path, key: UpdaterKey, version: str = VERSION, tag: str = TAG) -> None:
    payloads = [directory / name for name in release_assets.updater_payloads(version)]
    previous = os.getcwd()
    with contextlib.redirect_stdout(io.StringIO()):
        release_signing.sign_payloads(payloads, version, MINISIGN, key.env())
        release_signing.sign_sums(directory, version, MINISIGN, key.env())
        os.chdir(directory)
        try:
            with mock.patch.dict(os.environ, {"VERSION": version, "TAG": tag, "REPO": REPO}):
                if build_latest_json.main() != 0:
                    raise AssertionError("build_latest_json refused the signed release")
        finally:
            os.chdir(previous)


def serve_the_appimage_to_deb_installs(directory: pathlib.Path) -> None:
    manifest_path = directory / "latest.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["platforms"]["linux-x86_64-deb"] = manifest["platforms"]["linux-x86_64"]
    manifest_path.write_text(json.dumps(manifest))


def quietly(function, *args):
    with contextlib.redirect_stdout(io.StringIO()):
        return function(*args)


class ParsingTests(unittest.TestCase):
    def test_the_shipped_updater_key_reads_as_key_e67ad11d96ce21f6(self) -> None:
        key = release_signing.public_key_from_config(REPO_ROOT / "src-tauri" / "tauri.conf.json")
        self.assertEqual(key.key_id, "E67AD11D96CE21F6")

    def test_install_sh_embeds_the_shipped_updater_key(self) -> None:
        key = release_signing.public_key_from_config(REPO_ROOT / "src-tauri" / "tauri.conf.json")
        self.assertIn(f'MINISIGN_PUBKEY="{key.line}"', (REPO_ROOT / "install.sh").read_text())

    def test_the_trusted_comment_is_the_one_tauri_writes(self) -> None:
        comment = release_signing.trusted_comment("Writ_0.6.0_amd64.deb", "0.6.0", timestamp=1791801688)
        self.assertEqual(comment, "timestamp:1791801688\tfile:Writ_0.6.0_amd64.deb\tversion:0.6.0")
        self.assertEqual(
            release_signing.parse_trusted_comment(comment),
            {"timestamp": "1791801688", "file": "Writ_0.6.0_amd64.deb", "version": "0.6.0"},
        )

    def test_a_tab_or_line_break_cannot_reach_the_trusted_comment(self) -> None:
        for name, version in (("a\tb", "0.6.0"), ("a", "0.6\n0"), ("a\rb", "0.6.0")):
            with self.subTest(name=name, version=version):
                with self.assertRaises(release_signing.SigningError):
                    release_signing.trusted_comment(name, version)

    def test_a_minisign_failure_names_its_cause(self) -> None:
        def ran(code: int, stdout: str = "", stderr: str = "") -> subprocess.CompletedProcess[str]:
            return subprocess.CompletedProcess(["minisign"], code, stdout, stderr)

        self.assertEqual(release_signing.failure(ran(-9)), "minisign was killed by signal 9")
        self.assertEqual(
            release_signing.failure(ran(2, stderr="Password: \nWrong password for that key\n")),
            "Wrong password for that key",
        )
        self.assertEqual(release_signing.failure(ran(1)), "minisign exited with status 1 and no output")

    def test_a_signature_from_the_0_5_1_release_reads_as_the_shipped_key(self) -> None:
        shipped = (
            "untrusted comment: signature from tauri secret key\n"
            "RUT2Ic6WHdF65hPcJgK70xIkXRvMtNWquUYljEJBz1mB7EHFYALLTJmL29+4ONAASYSVAB7ZWuGdyDsE8ofEi89CBJwx6CXk5Qg=\n"
            "trusted comment: timestamp:1791801688\tfile:Writ.app.tar.gz\tversion:0.5.1\n"
            "5hW16A9s8fmOqG6o/Gh9TihJ1EACWKki+2YGSMDwhYUrSffH8W9icABw+dCRvJAVMAhVzbazFiLy82LNWXdyBg==\n"
        )
        self.assertEqual(release_signing.signature_key_id(shipped), "E67AD11D96CE21F6")
        encoded = release_signing.tauri_signature(shipped.encode())
        self.assertEqual(release_signing.minisig_from_tauri_signature(encoded), shipped)

    def test_a_public_key_that_is_not_minisign_is_refused(self) -> None:
        for text in ("", "RWT2Ic6WHdF65qf9kASdZd1XZWkUJbTZGCCEcnYK5DlCSDbKNEQX/OHk", "untrusted comment: x\nnot-base64!"):
            with self.subTest(text=text):
                with self.assertRaises(release_signing.SigningError):
                    release_signing.public_key_from_text(text)

    def test_a_config_without_an_updater_key_is_refused(self) -> None:
        with scratch() as tmp:
            config = tmp / "tauri.conf.json"
            config.write_text(json.dumps({"plugins": {}}))
            with self.assertRaises(release_signing.SigningError):
                release_signing.public_key_from_config(config)

    def test_sums_are_sorted_two_space_lines_that_parse_back(self) -> None:
        with scratch() as tmp:
            (tmp / "b.bin").write_bytes(b"b")
            (tmp / "a.bin").write_bytes(b"a")
            text = release_signing.sums_text(tmp, ["b.bin", "a.bin"])
        self.assertEqual(
            text,
            "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb  a.bin\n"
            "3e23e8160039594a33894f6564e1b1348bbd7a0088d42c4acb73eeaed59c009d  b.bin\n",
        )
        self.assertEqual(sorted(release_signing.parse_sums(text)), ["a.bin", "b.bin"])

    def test_a_malformed_or_repeated_sums_line_is_refused(self) -> None:
        digest = "0" * 64
        for text in (f"{digest} a.bin\n", f"{digest[:-1]}  a.bin\n", f"{digest}  a.bin\n{digest}  a.bin\n"):
            with self.subTest(text=text):
                with self.assertRaises(release_signing.SigningError):
                    release_signing.parse_sums(text)


class SecretKeyTests(unittest.TestCase):
    def test_the_decoded_key_is_private_and_removed_afterwards(self) -> None:
        key_text = "untrusted comment: rsign encrypted secret key\nRWRTY0Iy\n"
        env = {release_signing.KEY_ENV: base64.b64encode(key_text.encode()).decode()}
        with release_signing.secret_key_file(env) as path:
            self.assertEqual(path.read_text(), key_text)
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
            directory = path.parent
        self.assertFalse(directory.exists())

    def test_the_key_may_name_a_file(self) -> None:
        key_text = "untrusted comment: rsign encrypted secret key\nRWRTY0Iy\n"
        with scratch() as tmp:
            stored = tmp / "writ.key"
            stored.write_text(base64.b64encode(key_text.encode()).decode())
            with release_signing.secret_key_file({release_signing.KEY_ENV: str(stored)}) as path:
                self.assertEqual(path.read_text(), key_text)

    def test_an_empty_or_undecodable_key_is_refused(self) -> None:
        not_a_key = base64.b64encode(b"hello").decode()
        for value in ("", "   ", "not base64!", not_a_key, "A" * 5000):
            with self.subTest(value=value[:20]):
                with self.assertRaises(release_signing.SigningError):
                    with release_signing.secret_key_file({release_signing.KEY_ENV: value}):
                        pass

    def test_a_password_with_a_line_break_is_refused(self) -> None:
        with self.assertRaises(release_signing.SigningError):
            release_signing.password_line({release_signing.PASSWORD_ENV: "one\ntwo"})
        self.assertEqual(release_signing.password_line({}), "\n")


@requires_minisign
class SigningTests(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="writ-signing-test-")
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.keys = self.root / "keys"
        self.keys.mkdir()
        self.assets = self.root / "assets"
        self.assets.mkdir()
        self.key = UpdaterKey(self.keys, "release")

    def test_a_payload_signature_has_the_tauri_format(self) -> None:
        payload = self.assets / "Writ_0.6.0_amd64.deb"
        payload.write_bytes(b"deb bytes")
        quietly(release_signing.sign_payloads, [payload], VERSION, MINISIGN, self.key.env())

        encoded = (self.assets / "Writ_0.6.0_amd64.deb.sig").read_text()
        self.assertNotIn("\n", encoded)
        minisig = release_signing.minisig_from_tauri_signature(encoded)
        lines = minisig.splitlines()
        self.assertEqual(lines[0], "untrusted comment: signature from tauri secret key")
        self.assertTrue(lines[1].startswith("RU"), "not a prehashed (ED) signature")
        self.assertRegex(lines[2], r"^trusted comment: timestamp:\d+\tfile:Writ_0\.6\.0_amd64\.deb\tversion:0\.6\.0$")
        self.assertEqual(release_signing.signature_key_id(minisig), self.key.key_id)

    def test_a_signed_payload_verifies_against_its_key_and_version(self) -> None:
        payload = self.assets / "Writ_0.6.0_amd64.AppImage"
        payload.write_bytes(b"appimage bytes")
        quietly(release_signing.sign_payloads, [payload], VERSION, MINISIGN, self.key.env())
        public = release_signing.public_key_from_config(self.key.config)
        quietly(release_signing.verify_payloads, [payload], VERSION, public, MINISIGN)

    def test_a_changed_payload_fails_verification(self) -> None:
        payload = self.assets / "Writ_0.6.0_amd64.AppImage"
        payload.write_bytes(b"appimage bytes")
        quietly(release_signing.sign_payloads, [payload], VERSION, MINISIGN, self.key.env())
        payload.write_bytes(b"appimage bytes, replaced")
        public = release_signing.public_key_from_config(self.key.config)
        with self.assertRaisesRegex(release_signing.SigningError, "does not verify"):
            quietly(release_signing.verify_payloads, [payload], VERSION, public, MINISIGN)

    def test_a_signature_bound_to_another_version_fails(self) -> None:
        payload = self.assets / "Writ_0.6.0_x64_en-US.msi"
        payload.write_bytes(b"msi bytes")
        quietly(release_signing.sign_payloads, [payload], "0.5.1", MINISIGN, self.key.env())
        public = release_signing.public_key_from_config(self.key.config)
        with self.assertRaisesRegex(release_signing.SigningError, "bound to version '0.5.1'"):
            quietly(release_signing.verify_payloads, [payload], VERSION, public, MINISIGN)

    def test_a_password_protected_key_signs_with_its_password(self) -> None:
        key = UpdaterKey(self.keys, "protected", "correct horse")
        payload = self.assets / "Writ_0.6.0_amd64.deb"
        payload.write_bytes(b"deb bytes")
        quietly(release_signing.sign_payloads, [payload], VERSION, MINISIGN, key.env())
        public = release_signing.public_key_from_config(key.config)
        quietly(release_signing.verify_payloads, [payload], VERSION, public, MINISIGN)

    def test_a_wrong_password_signs_nothing(self) -> None:
        key = UpdaterKey(self.keys, "protected", "correct horse")
        payload = self.assets / "Writ_0.6.0_amd64.deb"
        payload.write_bytes(b"deb bytes")
        env = dict(key.env(), **{release_signing.PASSWORD_ENV: "wrong"})
        with self.assertRaisesRegex(release_signing.SigningError, "could not sign Writ_0.6.0_amd64.deb"):
            quietly(release_signing.sign_payloads, [payload], VERSION, MINISIGN, env)
        self.assertFalse((self.assets / "Writ_0.6.0_amd64.deb.sig").exists())

    def test_the_signed_sums_verify_with_the_command_users_run(self) -> None:
        write_release_inputs(self.assets)
        payloads = [self.assets / name for name in release_assets.updater_payloads(VERSION)]
        quietly(release_signing.sign_payloads, payloads, VERSION, MINISIGN, self.key.env())
        quietly(release_signing.sign_sums, self.assets, VERSION, MINISIGN, self.key.env())

        sums = (self.assets / "SHA256SUMS.txt").read_text()
        self.assertEqual(sorted(release_signing.parse_sums(sums)), release_assets.asset_set(VERSION, "checksummed"))
        public = release_signing.public_key_from_config(self.key.config)
        result = subprocess.run(
            [MINISIGN, "-Vm", "SHA256SUMS.txt", "-P", public.line],
            cwd=self.assets,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("file:SHA256SUMS.txt\tversion:0.6.0", result.stdout)
        checker = ["sha256sum", "-c"] if shutil.which("sha256sum") else ["shasum", "-a", "256", "-c"]
        checked = subprocess.run(
            [*checker, "SHA256SUMS.txt"],
            cwd=self.assets,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(checked.returncode, 0, checked.stdout + checked.stderr)

    def test_the_sums_refuse_a_file_outside_the_release(self) -> None:
        write_release_inputs(self.assets)
        payloads = [self.assets / name for name in release_assets.updater_payloads(VERSION)]
        quietly(release_signing.sign_payloads, payloads, VERSION, MINISIGN, self.key.env())
        (self.assets / "extra.exe").write_bytes(b"x")
        with self.assertRaisesRegex(release_signing.SigningError, "extra.exe"):
            quietly(release_signing.sign_sums, self.assets, VERSION, MINISIGN, self.key.env())
        self.assertFalse((self.assets / "SHA256SUMS.txt.minisig").exists())

    def test_the_sums_refuse_a_missing_signature(self) -> None:
        write_release_inputs(self.assets)
        with self.assertRaisesRegex(release_signing.SigningError, r"\.sig"):
            quietly(release_signing.sign_sums, self.assets, VERSION, MINISIGN, self.key.env())

    def test_a_signed_release_verifies_end_to_end(self) -> None:
        write_release_inputs(self.assets)
        sign_release(self.assets, self.key)
        public = release_signing.public_key_from_config(self.key.config)
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            release_signing.verify_release(self.assets, VERSION, TAG, REPO, public, MINISIGN)
        self.assertIn("every platform serves a signed 0.6.0 payload", output.getvalue())

    def test_a_tampered_release_fails_verification(self) -> None:
        public = release_signing.public_key_from_config(self.key.config)
        tamperings = {
            "installer bytes": lambda d: (d / "Writ_0.6.0_universal.pkg").write_bytes(b"other"),
            "sbom bytes": lambda d: (d / "writ-frontend_0.6.0.cdx.json").write_text("{}"),
            "manifest version": lambda d: (d / "latest.json").write_text(
                (d / "latest.json").read_text().replace('"version": "0.6.0"', '"version": "0.6.1"')
            ),
            "swapped platform entry": serve_the_appimage_to_deb_installs,
            "stray file": lambda d: (d / "notes.txt").write_text("x"),
            "missing minisig": lambda d: (d / "SHA256SUMS.txt.minisig").unlink(),
        }
        for label, tamper in tamperings.items():
            with self.subTest(tampering=label), scratch() as tmp:
                write_release_inputs(tmp)
                sign_release(tmp, self.key)
                tamper(tmp)
                with self.assertRaises(release_signing.SigningError):
                    quietly(release_signing.verify_release, tmp, VERSION, TAG, REPO, public, MINISIGN)

    def test_the_command_line_signs_and_verifies(self) -> None:
        write_release_inputs(self.assets)
        payloads = [str(self.assets / name) for name in release_assets.updater_payloads(VERSION)]
        key = UpdaterKey(self.keys, "protected", "correct horse")
        env = dict(os.environ, **key.env())
        script = str(REPO_ROOT / ".github" / "scripts" / "release_signing.py")
        output = ""
        for args in (
            ["sign-payloads", "--version", VERSION, *payloads],
            ["sign-sums", "--version", VERSION, "--dir", str(self.assets)],
            ["verify-payloads", "--version", VERSION, "--pubkey-from", str(key.config), *payloads],
        ):
            with self.subTest(command=args[0]):
                result = subprocess.run(
                    [sys.executable, script, "--minisign", MINISIGN, *args],
                    env=env,
                    capture_output=True,
                    text=True,
                    check=False,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                output += result.stdout + result.stderr
        self.assertNotIn(key.password, output)


@requires_minisign
class KeyTransitionRehearsal(unittest.TestCase):
    """Release N is signed with the old key and carries the new public key.

    Installed copies verify an update against the key built into them, which is
    the key of the latest published release, so the release workflow verifies
    against that key: N-1's for release N, N's for release N+1.
    """

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="writ-transition-test-")
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        keys = self.root / "keys"
        keys.mkdir()
        self.old = UpdaterKey(keys, "old")
        self.new = UpdaterKey(keys, "new")
        self.assertNotEqual(self.old.key_id, self.new.key_id)

    def release(self, label: str, version: str, key: UpdaterKey) -> pathlib.Path:
        directory = self.root / label
        directory.mkdir()
        write_release_inputs(directory, version)
        sign_release(directory, key, version, f"v{version}")
        return directory

    def verify(self, directory: pathlib.Path, version: str, trusted_config: pathlib.Path) -> None:
        public = release_signing.public_key_from_config(trusted_config)
        quietly(release_signing.verify_release, directory, version, f"v{version}", REPO, public, MINISIGN)

    def test_release_n_signed_with_the_old_key_reaches_copies_on_n_minus_1(self) -> None:
        release_n = self.release("n", "0.6.0", self.old)
        # The latest published release, N-1, embeds the old key.
        self.verify(release_n, "0.6.0", self.old.config)

    def test_release_n_plus_1_signed_with_the_new_key_reaches_copies_on_n(self) -> None:
        release_n_plus_1 = self.release("n1", "0.6.1", self.new)
        # Release N is now the latest published release and embeds the new key.
        self.verify(release_n_plus_1, "0.6.1", self.new.config)

    def test_swapping_the_secret_before_release_n_is_refused(self) -> None:
        early = self.release("early", "0.6.0", self.new)
        with self.assertRaisesRegex(
            release_signing.SigningError, f"signed with key {self.new.key_id}.*trust key {self.old.key_id}"
        ):
            self.verify(early, "0.6.0", self.old.config)

    def test_keeping_the_old_secret_after_release_n_is_refused(self) -> None:
        late = self.release("late", "0.6.1", self.old)
        with self.assertRaisesRegex(
            release_signing.SigningError, f"signed with key {self.old.key_id}.*trust key {self.new.key_id}"
        ):
            self.verify(late, "0.6.1", self.new.config)

    def test_a_copy_that_skipped_release_n_cannot_verify_n_plus_1(self) -> None:
        release_n_plus_1 = self.release("skip", "0.6.1", self.new)
        with self.assertRaises(release_signing.SigningError):
            self.verify(release_n_plus_1, "0.6.1", self.old.config)

    def test_the_manifest_of_release_n_carries_old_key_signatures_for_its_own_version(self) -> None:
        release_n = self.release("manifest", "0.6.0", self.old)
        manifest = json.loads((release_n / "latest.json").read_text())
        self.assertEqual(manifest["version"], "0.6.0")
        for platform, entry in manifest["platforms"].items():
            with self.subTest(platform=platform):
                minisig = release_signing.minisig_from_tauri_signature(entry["signature"])
                self.assertEqual(release_signing.signature_key_id(minisig), self.old.key_id)
                self.assertRegex(minisig.splitlines()[2], r"\tversion:0\.6\.0$")


@requires_minisign
@unittest.skipUnless(TAURI_CLI.exists(), "the Tauri CLI is not installed (pnpm install)")
class TauriKeyCompatibilityTests(unittest.TestCase):
    """The production key came from `tauri signer generate`; minisign must sign with that format."""

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="writ-tauri-key-test-")
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.key_path = self.root / "writ.key"
        subprocess.run(
            [str(TAURI_CLI), "signer", "generate", "--ci", "-p", "tauri password", "-w", str(self.key_path)],
            capture_output=True,
            text=True,
            check=True,
            timeout=120,
        )
        public_text = base64.b64decode(self.key_path.with_suffix(".key.pub").read_text()).decode()
        self.config = self.root / "tauri.conf.json"
        self.config.write_text(
            json.dumps({"plugins": {"updater": {"pubkey": base64.b64encode(public_text.encode()).decode()}}})
        )
        self.env = {
            release_signing.KEY_ENV: self.key_path.read_text(),
            release_signing.PASSWORD_ENV: "tauri password",
        }

    def test_minisign_signs_with_a_tauri_generated_key(self) -> None:
        payload = self.root / "Writ_0.6.0_amd64.deb"
        payload.write_bytes(b"deb bytes")
        quietly(release_signing.sign_payloads, [payload], VERSION, MINISIGN, self.env)
        public = release_signing.public_key_from_config(self.config)
        quietly(release_signing.verify_payloads, [payload], VERSION, public, MINISIGN)

    def test_a_tauri_signer_signature_verifies_the_same_way(self) -> None:
        payload = self.root / "Writ_0.6.0_amd64.AppImage"
        payload.write_bytes(b"appimage bytes")
        env = dict(
            os.environ,
            TAURI_SIGNING_PRIVATE_KEY=self.env[release_signing.KEY_ENV],
            TAURI_SIGNING_PRIVATE_KEY_PASSWORD="tauri password",
        )
        subprocess.run(
            [str(TAURI_CLI), "signer", "sign", "--app-version", VERSION, str(payload)],
            env=env,
            capture_output=True,
            text=True,
            check=True,
            timeout=120,
        )
        ours = self.root / "ours"
        ours.mkdir()
        copy = ours / payload.name
        copy.write_bytes(payload.read_bytes())
        quietly(release_signing.sign_payloads, [copy], VERSION, MINISIGN, self.env)

        theirs = release_signing.minisig_from_tauri_signature((self.root / f"{payload.name}.sig").read_text())
        mine = release_signing.minisig_from_tauri_signature((ours / f"{payload.name}.sig").read_text())
        shape = re.compile(r"timestamp:\d+")
        self.assertEqual(theirs.splitlines()[0], mine.splitlines()[0])
        self.assertEqual(shape.sub("timestamp:T", theirs.splitlines()[2]), shape.sub("timestamp:T", mine.splitlines()[2]))
        public = release_signing.public_key_from_config(self.config)
        quietly(release_signing.verify_payloads, [payload], VERSION, public, MINISIGN)


if __name__ == "__main__":
    unittest.main()
