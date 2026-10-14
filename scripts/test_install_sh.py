"""Tests for install.sh with a stubbed host, a stubbed download and a test key.

The release key at install.sh:9 is replaced with a key generated per run, and
the fixture release is signed with it the way release_signing.py signs a real
one (minisign's prehashed Ed25519, base64-wrapped for the AppImage). Signing
needs OpenSSL 3.0 or later: WRIT_TEST_OPENSSL, then `openssl` on PATH, then
Homebrew's openssl@3. The minisign case also needs minisign: MINISIGN, then
`minisign` on PATH.

Run from the repository root:
    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import base64
import hashlib
import os
import pathlib
import re
import shutil
import subprocess
import tempfile
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = REPO_ROOT / "install.sh"
KEY_LINE = re.compile(r'^MINISIGN_PUBKEY="[A-Za-z0-9+/=]+"$', re.MULTILINE)
VERSION = "9.9.9"
ASSET = f"Writ_{VERSION}_amd64.AppImage"

# Tools install.sh calls, linked one by one so a test decides whether
# `openssl` and `minisign` exist.
HOST_TOOLS = (
    "sh", "awk", "sed", "tr", "base64", "dd", "od", "wc", "cat", "cp", "rm", "mkdir",
    "chmod", "install", "mktemp", "head", "grep", "printf", "env",
)

UNAME = """#!/bin/sh
case "$1" in
  -m) echo x86_64 ;;
  *) echo Linux ;;
esac
"""

# -I prints the redirect to the release tag; -o copies the fixture named by the
# URL's last segment, and fails like curl -f when there is none.
CURL = """#!/bin/sh
printf '%s\\n' "$*" >> "$CURL_LOG"
out=""
head=0
url=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) out="$2"; shift ;;
    -fsSI) head=1 ;;
    http*) url="$1" ;;
  esac
  shift
done
if [ "$head" = 1 ]; then
  printf 'HTTP/2 302\\r\\nlocation: https://github.com/ibrahemid/writ/releases/tag/v%s\\r\\n\\r\\n' "$FAKE_VERSION"
  exit 0
fi
name="${url##*/}"
[ -f "$FIXTURES/$name" ] || exit 22
cp "$FIXTURES/$name" "$out"
"""

FAKE_APPIMAGE = """#!/bin/sh
mkdir -p squashfs-root/usr/bin
printf '#!/bin/sh\\necho writ\\n' > squashfs-root/usr/bin/writ
"""

OLD_OPENSSL = """#!/bin/sh
[ "$1" = version ] && echo "OpenSSL 1.1.1w  11 Sep 2023" && exit 0
exit 1
"""


def find_openssl3() -> str | None:
    candidates = [
        os.environ.get("WRIT_TEST_OPENSSL"),
        shutil.which("openssl"),
        "/opt/homebrew/opt/openssl@3/bin/openssl",
        "/usr/local/opt/openssl@3/bin/openssl",
    ]
    for candidate in candidates:
        if not candidate or not os.access(candidate, os.X_OK):
            continue
        version = subprocess.run([candidate, "version"], capture_output=True, text=True).stdout
        match = re.match(r"OpenSSL (\d+)\.", version)
        if match and int(match.group(1)) >= 3:
            return candidate
    return None


def find_minisign() -> str | None:
    candidate = os.environ.get("MINISIGN") or shutil.which("minisign")
    return candidate if candidate and os.access(candidate, os.X_OK) else None


OPENSSL = find_openssl3()
MINISIGN = find_minisign()


class TestKey:
    """An Ed25519 key in minisign's format, made and used through OpenSSL."""

    def __init__(self, directory: pathlib.Path, key_id: bytes) -> None:
        assert OPENSSL is not None
        self.dir = directory
        self.dir.mkdir()
        self.signing_key_pem = self.dir / "signing-key.pem"
        subprocess.run(
            [OPENSSL, "genpkey", "-algorithm", "ed25519", "-out", str(self.signing_key_pem)],
            check=True,
            capture_output=True,
        )
        der = subprocess.run(
            [OPENSSL, "pkey", "-in", str(self.signing_key_pem), "-pubout", "-outform", "DER"],
            check=True,
            capture_output=True,
        ).stdout
        self.key_id = key_id
        self.public = base64.b64encode(b"Ed" + key_id + der[-32:]).decode()

    def raw_sign(self, message: bytes) -> bytes:
        message_path = self.dir / "message"
        message_path.write_bytes(message)
        return subprocess.run(
            [OPENSSL, "pkeyutl", "-sign", "-inkey", str(self.signing_key_pem), "-rawin", "-in", str(message_path)],
            check=True,
            capture_output=True,
        ).stdout

    def minisig(self, data: bytes, trusted: str) -> str:
        digest = hashlib.blake2b(data, digest_size=64).digest()
        signature = self.raw_sign(digest)
        global_signature = self.raw_sign(signature + trusted.encode())
        return (
            "untrusted comment: signature from a test key\n"
            + base64.b64encode(b"ED" + self.key_id + signature).decode()
            + f"\ntrusted comment: {trusted}\n"
            + base64.b64encode(global_signature).decode()
            + "\n"
        )


@unittest.skipIf(OPENSSL is None, "signing the fixtures needs OpenSSL 3.0 or later")
class InstallScriptTests(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="writ-install-sh-test-")
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.key = TestKey(self.root / "key", b"\x01\x02\x03\x04\x05\x06\x07\x08")
        self.other_key = TestKey(self.root / "other-key", b"\x01\x02\x03\x04\x05\x06\x07\x08")

        script = KEY_LINE.sub(f'MINISIGN_PUBKEY="{self.key.public}"', SCRIPT.read_text(), count=1)
        self.assertIn(self.key.public, script)
        self.script = self.root / "install.sh"
        self.script.write_text(script)

        self.tools = self.root / "tools"
        self.tools.mkdir()
        host_path = "/usr/bin:/bin:/usr/sbin:/sbin"
        for tool in HOST_TOOLS + (("sha256sum",) if shutil.which("sha256sum", path=host_path) else ("shasum",)):
            found = shutil.which(tool, path=host_path)
            self.assertIsNotNone(found, f"{tool} is not on {host_path}")
            (self.tools / tool).symlink_to(found)
        for name, body in {"uname": UNAME, "curl": CURL}.items():
            stub = self.tools / name
            stub.write_text(body)
            stub.chmod(0o755)

        self.fixtures = self.root / "release"
        self.fixtures.mkdir()
        self.home = self.root / "home"
        self.home.mkdir()
        self.curl_log = self.root / "curl.log"
        self.env = {
            "PATH": str(self.tools),
            "HOME": str(self.home),
            "FIXTURES": str(self.fixtures),
            "FAKE_VERSION": VERSION,
            "CURL_LOG": str(self.curl_log),
        }
        self.publish(FAKE_APPIMAGE.encode())

    def publish(
        self,
        appimage: bytes,
        sums_key: TestKey | None = None,
        asset_key: TestKey | None = None,
        listed_name: str = ASSET,
    ) -> None:
        """Writes a release: the AppImage, its .sig, the sums and their signature."""
        (self.fixtures / ASSET).write_bytes(appimage)
        sums = f"{hashlib.sha256(appimage).hexdigest()}  {listed_name}\n".encode()
        (self.fixtures / "SHA256SUMS.txt").write_bytes(sums)
        (self.fixtures / "SHA256SUMS.txt.minisig").write_text(
            (sums_key or self.key).minisig(sums, f"timestamp:1\tfile:SHA256SUMS.txt\tversion:{VERSION}")
        )
        asset_sig = (asset_key or self.key).minisig(appimage, f"timestamp:1\tfile:{ASSET}\tversion:{VERSION}")
        (self.fixtures / f"{ASSET}.sig").write_text(base64.b64encode(asset_sig.encode()).decode())

    def use(self, tool: str, path: str) -> None:
        (self.tools / tool).symlink_to(path)

    def stub(self, tool: str, body: str) -> None:
        path = self.tools / tool
        path.write_text(body)
        path.chmod(0o755)

    def run_install(self, **extra: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["/bin/sh", str(self.script)],
            env={**self.env, **extra},
            capture_output=True,
            text=True,
            cwd=self.root,
        )

    def downloads(self) -> list[str]:
        if not self.curl_log.exists():
            return []
        return [line.rsplit("/", 1)[-1] for line in self.curl_log.read_text().splitlines() if " -o " in line]

    def assert_installed(self, result: subprocess.CompletedProcess[str]) -> None:
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f"installed {VERSION}", result.stdout)
        self.assertTrue((self.home / ".local/share/writ/Writ.AppImage").is_file())
        self.assertTrue((self.home / ".local/bin/writ").is_file())

    def assert_refused(self, result: subprocess.CompletedProcess[str], message: str) -> None:
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn(message, result.stderr)
        self.assertFalse((self.home / ".local/share/writ/Writ.AppImage").exists())
        self.assertFalse((self.home / ".local/bin/writ").exists())

    def test_installs_a_release_openssl_verifies(self) -> None:
        self.use("openssl", OPENSSL)  # type: ignore[arg-type]
        result = self.run_install()
        self.assert_installed(result)
        self.assertIn("SHA256SUMS.txt.minisig", self.downloads())
        self.assertIn(f"{ASSET}.sig", self.downloads())

    @unittest.skipIf(MINISIGN is None, "minisign is not installed")
    def test_installs_a_release_minisign_verifies(self) -> None:
        self.use("minisign", MINISIGN)  # type: ignore[arg-type]
        self.assert_installed(self.run_install())

    @unittest.skipIf(MINISIGN is None, "minisign is not installed")
    def test_minisign_refuses_sums_signed_by_another_key(self) -> None:
        self.use("minisign", MINISIGN)  # type: ignore[arg-type]
        self.publish(FAKE_APPIMAGE.encode(), sums_key=self.other_key)
        self.assert_refused(self.run_install(), "the signature on SHA256SUMS.txt does not match")

    def test_refuses_a_download_whose_checksum_does_not_match(self) -> None:
        self.use("openssl", OPENSSL)  # type: ignore[arg-type]
        (self.fixtures / ASSET).write_bytes(FAKE_APPIMAGE.encode() + b"# tampered\n")
        self.assert_refused(self.run_install(), f"checksum mismatch for {ASSET}")

    def test_refuses_an_asset_the_sums_do_not_list(self) -> None:
        self.use("openssl", OPENSSL)  # type: ignore[arg-type]
        self.publish(FAKE_APPIMAGE.encode(), listed_name="Writ_9.9.8_amd64.AppImage")
        self.assert_refused(self.run_install(), f"{ASSET} is not listed in SHA256SUMS.txt")

    def test_refuses_sums_signed_by_another_key(self) -> None:
        self.use("openssl", OPENSSL)  # type: ignore[arg-type]
        self.publish(FAKE_APPIMAGE.encode(), sums_key=self.other_key)
        self.assert_refused(self.run_install(), "the signature on SHA256SUMS.txt does not match")
        self.assertNotIn(f"{ASSET}.sig", self.downloads())

    def test_refuses_sums_whose_trusted_comment_was_changed(self) -> None:
        self.use("openssl", OPENSSL)  # type: ignore[arg-type]
        signature = self.fixtures / "SHA256SUMS.txt.minisig"
        signature.write_text(signature.read_text().replace("version:9.9.9", "version:9.9.10"))
        self.assert_refused(self.run_install(), "the signature on SHA256SUMS.txt does not match")

    def test_refuses_an_appimage_signed_by_another_key(self) -> None:
        self.use("openssl", OPENSSL)  # type: ignore[arg-type]
        self.publish(FAKE_APPIMAGE.encode(), asset_key=self.other_key)
        self.assert_refused(self.run_install(), f"signature check failed for {ASSET}")

    def test_fails_closed_without_a_verifier(self) -> None:
        result = self.run_install()
        self.assert_refused(result, "install minisign or OpenSSL 3.0 or later")
        self.assertIn("WRIT_INSTALL_SKIP_SIGNATURE=1", result.stderr)
        self.assertEqual(self.downloads(), [])

    def test_an_openssl_older_than_3_is_no_verifier(self) -> None:
        self.stub("openssl", OLD_OPENSSL)
        self.assert_refused(self.run_install(), "install minisign or OpenSSL 3.0 or later")

    def test_the_escape_installs_without_a_signature_check_and_says_so(self) -> None:
        result = self.run_install(WRIT_INSTALL_SKIP_SIGNATURE="1")
        self.assert_installed(result)
        self.assertIn("the release signature is not checked", result.stderr)
        self.assertNotIn("SHA256SUMS.txt.minisig", self.downloads())

    def test_the_escape_still_checks_the_checksum(self) -> None:
        (self.fixtures / ASSET).write_bytes(FAKE_APPIMAGE.encode() + b"# tampered\n")
        self.assert_refused(self.run_install(WRIT_INSTALL_SKIP_SIGNATURE="1"), "checksum mismatch")


if __name__ == "__main__":
    unittest.main()
