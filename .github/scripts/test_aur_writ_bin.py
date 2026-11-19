"""Build and install writ-bin in an Arch container from release assets, over the previous release.

The release workflow runs this on an Ubuntu runner with Docker:
    AUR_APPIMAGE=<Jotra AppImage> AUR_APPIMAGE_SIG=<its .sig> AUR_VERSION=<version> \\
    AUR_PREVIOUS_PKGBUILD=<previous PKGBUILD> AUR_PREVIOUS_APPIMAGE=<Writ AppImage> \\
    AUR_PREVIOUS_APPIMAGE_SIG=<its .sig> AUR_PREVIOUS_VERSION=<previous version> \\
        python3 .github/scripts/test_aur_writ_bin.py

Each PKGBUILD is bumped with scripts/packaging_bump_aur.py, as the release does,
and its source array is pointed at the local files, because the assets are not
at their release URLs until the release is published. AUR_PKGBUILD and
AUR_LICENSE replace the repository's PKGBUILD and LICENSE; AUR_MINISIGN_PUBKEY
replaces the key in the new PKGBUILD only; AUR_IMAGE names the Arch image.
Without the six variables the tests are skipped. Run as a script, a skipped
test fails the run.
"""

from __future__ import annotations

import dataclasses
import hashlib
import importlib.util
import os
import pathlib
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
import uuid

from check_process import ImmediateTextResult, run_bounded
from test_deb_upgrade_legacy import parse_desktop_entry

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
DEFAULT_PKGBUILD = REPO_ROOT / "packaging/aur/writ-bin/PKGBUILD"
DEFAULT_LICENSE = REPO_ROOT / "LICENSE"
BUMP_SCRIPT = REPO_ROOT / "scripts/packaging_bump_aur.py"
DEFAULT_IMAGE = "archlinux:base-devel"
DOCKER_TIMEOUT = 600.0
REMOVE_TIMEOUT = 120.0
PACKAGE = "writ-bin"
OPT = "/opt/jotra"
LEGACY_OPT = "/opt/writ"
SOURCE_ARRAY = re.compile(r"^source=\([^)]*\)", re.MULTILINE)
PUBKEY_LINE = re.compile(r"^_minisign_pubkey='[^']*'$", re.MULTILINE)
REQUIRED = ("AUR_APPIMAGE", "AUR_APPIMAGE_SIG", "AUR_VERSION")
PREVIOUS_REQUIRED = ("AUR_PREVIOUS_PKGBUILD", "AUR_PREVIOUS_APPIMAGE", "AUR_PREVIOUS_APPIMAGE_SIG", "AUR_PREVIOUS_VERSION")


class AurCheckError(RuntimeError):
    pass


@dataclasses.dataclass(frozen=True)
class Release:
    version: str
    pkgbuild: pathlib.Path
    appimage: pathlib.Path
    signature: pathlib.Path
    license: pathlib.Path
    pubkey: str


def load_bump_module():
    spec = importlib.util.spec_from_file_location("packaging_bump_aur", BUMP_SCRIPT)
    if spec is None or spec.loader is None:
        raise AurCheckError(f"cannot load {BUMP_SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


bump = load_bump_module()


def get_file(variable: str, default: pathlib.Path | None = None) -> pathlib.Path | None:
    value = os.environ.get(variable, "")
    if not value:
        return default
    path = pathlib.Path(value).resolve()
    if not path.is_file():
        raise AurCheckError(f"{variable} names {value}, which is not a file")
    return path


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def get_local_names(text: str) -> list[str]:
    names = [entry.split("::", 1)[0] for entry in bump.read_pkgbuild_array(text, "source")]
    if len(names) != 3:
        raise AurCheckError(f"the PKGBUILD lists {len(names)} sources, expected the AppImage, its signature and LICENSE: {names}")
    return names


def localize_pkgbuild(text: str, release: Release) -> tuple[str, list[str]]:
    text = bump.rewrite_pkgbuild(text, release.version, sha256(release.appimage), sha256(release.license))
    names = get_local_names(text)
    array = SOURCE_ARRAY.search(text)
    if array is None:
        raise AurCheckError("the PKGBUILD has no source array")
    entries = shlex.split(array.group(0).removeprefix("source=(").removesuffix(")"), comments=True)
    local = " ".join(f'"{entry.split("::", 1)[0]}"' for entry in entries)
    text = SOURCE_ARRAY.sub(lambda _: f"source=({local})", text, count=1)
    if release.pubkey:
        text, count = PUBKEY_LINE.subn(f"_minisign_pubkey='{release.pubkey}'", text, count=1)
        if count != 1:
            raise AurCheckError("the PKGBUILD has no _minisign_pubkey line to replace")
    return text, names


def stage_build(folder: pathlib.Path, release: Release) -> None:
    folder.mkdir(parents=True)
    text, names = localize_pkgbuild(release.pkgbuild.read_text(), release)
    (folder / "PKGBUILD").write_text(text)
    for name, source in zip(names, (release.appimage, release.signature, release.license)):
        shutil.copyfile(source, folder / name)


def parse_owner(text: str) -> str:
    match = re.search(r" is owned by (\S+) ", text)
    return match.group(1) if match else ""


class Container:
    def __init__(self, image: str) -> None:
        self.name = f"aur-check-{uuid.uuid4().hex[:12]}"
        self.image = image

    def start(self) -> None:
        self.run_host("docker", "run", "--detach", "--name", self.name, self.image, "sleep", "infinity")

    def stop(self) -> None:
        run_bounded(["docker", "rm", "--force", self.name], timeout=REMOVE_TIMEOUT)

    def run_host(self, *args: str) -> str:
        print(f"$ {shlex.join(args)}", flush=True)
        result = run_bounded(args, timeout=DOCKER_TIMEOUT)
        if result.returncode != 0:
            raise AurCheckError(f"{shlex.join(args)} exited {result.returncode}\n{result.stdout}{result.stderr}")
        return result.stdout

    def run(self, script: str, check: bool = True) -> subprocess.CompletedProcess[str]:
        args = ["docker", "exec", self.name, "bash", "-euo", "pipefail", "-c", script]
        print(f"$ {script}", flush=True)
        result = run_bounded(args, timeout=DOCKER_TIMEOUT)
        print(f"exit {result.returncode}\n{result.stdout}{result.stderr}", flush=True)
        if check and result.returncode != 0:
            raise AurCheckError(f"{script} exited {result.returncode} in {self.image}\n{result.stdout}{result.stderr}")
        return result

    def output(self, script: str) -> str:
        return self.run(script).stdout.strip()

    def build_and_install(self, host_folder: pathlib.Path, leg: str) -> None:
        target = f"/build/{leg}"
        self.run(f"mkdir -p /build && rm -rf {target}")
        self.run_host("docker", "cp", str(host_folder), f"{self.name}:{target}")
        self.run(f"chown -R builder {target}")
        self.run(f"runuser -u builder -- bash -c 'cd {target} && makepkg --nodeps --noconfirm'")
        package = self.output(f"find {target} -maxdepth 1 -name '{PACKAGE}-*.pkg.tar.*' ! -name '*-debug-*' -print -quit")
        if not package:
            raise AurCheckError(f"makepkg left no {PACKAGE} package in {target}")
        self.run(f"pacman -U --noconfirm {shlex.quote(package)}")


class ParseTests(unittest.TestCase):
    def make_release(self, folder: pathlib.Path, pubkey: str = "") -> Release:
        appimage = folder / "Jotra_9.9.9_amd64.AppImage"
        appimage.write_bytes(b"appimage")
        signature = folder / "Jotra_9.9.9_amd64.AppImage.sig"
        signature.write_text("sig")
        return Release("9.9.9", DEFAULT_PKGBUILD, appimage, signature, DEFAULT_LICENSE, pubkey)

    def read_with_bash(self, pkgbuild: pathlib.Path) -> list[str]:
        script = 'source "$1"; printf "%s\\n" "$pkgver" "$_minisign_pubkey" "${source[@]}" "${sha256sums[@]}"'
        result = subprocess.run(["bash", "-c", script, "bash", str(pkgbuild)], capture_output=True, text=True, check=True)
        return result.stdout.splitlines()

    def test_the_new_pkgbuild_builds_from_local_files(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = pathlib.Path(folder)
            release = self.make_release(root, "RWQtestkey")
            stage_build(root / "build", release)
            files = sorted(path.name for path in (root / "build").iterdir())
            self.assertEqual(files, ["LICENSE-9.9.9", "PKGBUILD", "jotra-9.9.9.AppImage", "jotra-9.9.9.AppImage.minisign"])
            self.assertEqual((root / "build/jotra-9.9.9.AppImage").read_bytes(), b"appimage")
            self.assertEqual(
                self.read_with_bash(root / "build/PKGBUILD"),
                [
                    "9.9.9",
                    "RWQtestkey",
                    "jotra-9.9.9.AppImage",
                    "jotra-9.9.9.AppImage.minisign",
                    "LICENSE-9.9.9",
                    sha256(release.appimage),
                    "SKIP",
                    sha256(DEFAULT_LICENSE),
                ],
            )

    def test_the_key_stays_when_no_override_is_given(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = pathlib.Path(folder)
            stage_build(root / "build", self.make_release(root))
            key = re.search(r"^_minisign_pubkey='([^']*)'$", DEFAULT_PKGBUILD.read_text(), re.MULTILINE).group(1)
            self.assertEqual(self.read_with_bash(root / "build/PKGBUILD")[1], key)

    def test_a_previous_pkgbuild_keeps_its_own_file_names(self) -> None:
        text = DEFAULT_PKGBUILD.read_text().replace("_pkgname=jotra", "_pkgname=writ").replace("Jotra_", "Writ_")
        with tempfile.TemporaryDirectory() as folder:
            root = pathlib.Path(folder)
            previous = root / "PKGBUILD.previous"
            previous.write_text(text)
            release = dataclasses.replace(self.make_release(root), pkgbuild=previous, version="1.0.1")
            stage_build(root / "build", release)
            self.assertEqual(
                self.read_with_bash(root / "build/PKGBUILD")[2:5],
                ["writ-1.0.1.AppImage", "writ-1.0.1.AppImage.minisign", "LICENSE-1.0.1"],
            )

    def test_the_owner_is_read_from_pacman(self) -> None:
        self.assertEqual(parse_owner("/usr/bin/writ is owned by writ-bin 1.0.2-1\n"), "writ-bin")
        self.assertEqual(parse_owner("error: No package owns /usr/bin/writ\n"), "")


class UpgradeTests(unittest.TestCase):
    container: Container
    work: pathlib.Path
    version: str
    previous_version: str
    previous_command: str

    @classmethod
    def setUpClass(cls) -> None:
        if not all(os.environ.get(name) for name in REQUIRED + PREVIOUS_REQUIRED):
            raise unittest.SkipTest(f"{', '.join(REQUIRED + PREVIOUS_REQUIRED)} are not all set")
        license_path = get_file("AUR_LICENSE", DEFAULT_LICENSE)
        pkgbuild = get_file("AUR_PKGBUILD", DEFAULT_PKGBUILD)
        assert license_path is not None and pkgbuild is not None
        current = Release(
            os.environ["AUR_VERSION"],
            pkgbuild,
            get_file("AUR_APPIMAGE"),
            get_file("AUR_APPIMAGE_SIG"),
            license_path,
            os.environ.get("AUR_MINISIGN_PUBKEY", ""),
        )
        previous = Release(
            os.environ["AUR_PREVIOUS_VERSION"],
            get_file("AUR_PREVIOUS_PKGBUILD"),
            get_file("AUR_PREVIOUS_APPIMAGE"),
            get_file("AUR_PREVIOUS_APPIMAGE_SIG"),
            license_path,
            "",
        )
        cls.version = current.version
        cls.previous_version = previous.version
        cls.work = pathlib.Path(tempfile.mkdtemp(prefix="aur-check-"))
        stage_build(cls.work / "previous", previous)
        stage_build(cls.work / "current", current)
        cls.container = Container(os.environ.get("AUR_IMAGE", DEFAULT_IMAGE))
        cls.container.start()
        try:
            cls.container.run("pacman -Syu --noconfirm --needed minisign")
            cls.container.run("useradd --create-home builder")
            cls.container.build_and_install(cls.work / "previous", "previous")
            installed = cls.container.output(f"pacman -Q {PACKAGE}")
            if installed != f"{PACKAGE} {previous.version}-1":
                raise AurCheckError(f"the previous build installed {installed!r}, expected {PACKAGE} {previous.version}-1")
            cls.previous_command = cls.container.output("/usr/bin/writ --version")
            cls.container.build_and_install(cls.work / "current", "current")
        except BaseException:
            cls.container.stop()
            shutil.rmtree(cls.work, ignore_errors=True)
            raise

    @classmethod
    def tearDownClass(cls) -> None:
        cls.container.stop()
        shutil.rmtree(cls.work, ignore_errors=True)

    def test_the_previous_release_was_installed_first(self) -> None:
        self.assertIn(self.previous_version, self.previous_command, f"/usr/bin/writ --version before the update: {self.previous_command}")

    def test_the_package_is_the_new_version_with_every_file(self) -> None:
        self.assertEqual(self.container.output(f"pacman -Q {PACKAGE}"), f"{PACKAGE} {self.version}-1")
        files = self.container.run(f"pacman -Qk {PACKAGE}", check=False)
        self.assertEqual(files.returncode, 0, f"pacman -Qk {PACKAGE}: {files.stdout}{files.stderr}")

    def test_the_previous_payload_is_gone(self) -> None:
        listing = self.container.output(f"ls -A /opt; test ! -e {LEGACY_OPT} && echo gone || echo kept")
        self.assertTrue(listing.endswith("gone"), f"{LEGACY_OPT} is still there after the update: {listing}")
        self.assertEqual(self.container.output(f"cat {OPT}/.package-manager"), "pacman")

    def test_both_commands_run_the_new_version_and_belong_to_the_package(self) -> None:
        for command in ("jotra", "writ"):
            with self.subTest(command=command):
                output = self.container.output(f"/usr/bin/{command} --version")
                self.assertIn(self.version, output, f"/usr/bin/{command} --version: {output}")
                owner = self.container.output(f"pacman -Qo /usr/bin/{command}")
                self.assertEqual(parse_owner(owner + " "), PACKAGE, f"pacman -Qo /usr/bin/{command}: {owner}")

    def test_the_legacy_launcher_links_to_the_new_one(self) -> None:
        self.assertEqual(self.container.output("readlink /usr/bin/writ-tauri"), "jotra-tauri")
        launcher = self.container.output("cat /usr/bin/jotra-tauri")
        self.assertIn(f'exec "{OPT}/AppRun" "$@"', launcher, f"/usr/bin/jotra-tauri: {launcher}")

    def test_the_desktop_entries_name_the_new_app_and_keep_the_old_id(self) -> None:
        current = parse_desktop_entry(self.container.output("cat /usr/share/applications/jotra.desktop"))
        legacy = parse_desktop_entry(self.container.output("cat /usr/share/applications/writ.desktop"))
        self.assertEqual(current.get("Exec"), f"{OPT}/AppRun %U", f"jotra.desktop: {current}")
        self.assertEqual(current.get("Icon"), "jotra", f"jotra.desktop: {current}")
        self.assertNotEqual(current.get("NoDisplay"), "true", f"jotra.desktop: {current}")
        self.assertEqual(legacy.get("NoDisplay"), "true", f"writ.desktop: {legacy}")
        self.assertEqual(legacy.get("Exec"), current.get("Exec"), f"writ.desktop: {legacy}")
        self.assertEqual(legacy.get("MimeType"), current.get("MimeType"), f"writ.desktop: {legacy}")

    def test_the_icons_and_license_are_installed(self) -> None:
        icons = self.container.output("find /usr/share/icons -name jotra.png")
        self.assertTrue(icons, "no jotra.png under /usr/share/icons")
        self.container.run(f"test -s /usr/share/licenses/{PACKAGE}/LICENSE")


def main() -> int:
    suite = unittest.defaultTestLoader.loadTestsFromModule(sys.modules[__name__])
    result = unittest.TextTestRunner(verbosity=2, resultclass=ImmediateTextResult).run(suite)
    for test, reason in result.skipped:
        print(f"skipped {test}: {reason}", file=sys.stderr)
    return 0 if result.wasSuccessful() and not result.skipped else 1


if __name__ == "__main__":
    sys.exit(main())
