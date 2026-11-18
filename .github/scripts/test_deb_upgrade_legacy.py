"""Install the previous release's deb, then the new deb over it with dpkg.

The release workflow runs this on a disposable Ubuntu runner, where it installs
packages with sudo:
    DEB_UPGRADE_FROM=<previous .deb> DEB_UPGRADE_TO=<new .deb> \\
        python3 .github/scripts/test_deb_upgrade_legacy.py

Without those variables the deb tests are skipped. Run as a script, a skipped
test fails the run.
"""

from __future__ import annotations

import dataclasses
import io
import os
import pathlib
import shutil
import subprocess
import sys
import tarfile
import unittest

LEGACY_PACKAGE = "writ"
PACKAGE = "jotra"
LEGACY_COMMAND_PATH = "usr/bin/writ"
COMMAND_PATH = "usr/bin/jotra"
LEGACY_LAUNCHER_PATH = "usr/bin/writ-tauri"
LAUNCHER_PATH = "usr/bin/jotra-tauri"
LEGACY_DESKTOP_PATH = "usr/share/applications/Writ.desktop"
DESKTOP_PATH = "usr/share/applications/Jotra.desktop"
RELATIONSHIP_FIELDS = ("Conflicts", "Replaces", "Provides")
FROM_VARIABLE = "DEB_UPGRADE_FROM"
TO_VARIABLE = "DEB_UPGRADE_TO"
GONE_STATUSES = {"not-installed", "config-files"}


class DebCheckError(RuntimeError):
    pass


@dataclasses.dataclass(frozen=True)
class DebEntry:
    kind: str
    target: str = ""


def get_deb_path(variable: str) -> pathlib.Path | None:
    value = os.environ.get(variable, "")
    if not value:
        return None
    path = pathlib.Path(value).resolve()
    if not path.is_file():
        raise DebCheckError(f"{variable} names {value}, which is not a file")
    return path


def run_command(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, capture_output=True, text=True, check=False)


def require_output(result: subprocess.CompletedProcess[str]) -> str:
    if result.returncode != 0:
        command = " ".join(str(arg) for arg in result.args)
        raise DebCheckError(f"{command} exited {result.returncode}: {result.stderr.strip()}")
    return result.stdout


def parse_control_fields(text: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    name: str | None = None
    for line in text.splitlines():
        if line[:1] in (" ", "\t"):
            if name is None:
                raise DebCheckError(f"continuation line before any field: {line!r}")
            fields[name] = f"{fields[name]}\n{line.strip()}"
        elif ":" in line:
            name, value = line.split(":", 1)
            fields[name] = value.strip()
        elif line.strip():
            raise DebCheckError(f"unexpected control line: {line!r}")
    return fields


def parse_deb_listing(text: str) -> dict[str, DebEntry]:
    entries: dict[str, DebEntry] = {}
    for line in text.splitlines():
        parts = line.split(None, 5)
        if len(parts) != 6:
            raise DebCheckError(f"unexpected dpkg-deb listing line: {line!r}")
        mode, name = parts[0], parts[5]
        if mode.startswith("l"):
            name, separator, target = name.partition(" -> ")
            if not separator:
                raise DebCheckError(f"link without a target: {line!r}")
            entry = DebEntry("link", target)
        elif mode.startswith("-"):
            entry = DebEntry("file")
        elif mode.startswith("d"):
            entry = DebEntry("directory")
        else:
            entry = DebEntry("other")
        entries[name.removeprefix("./").rstrip("/")] = entry
    return entries


def parse_desktop_entry(text: str) -> dict[str, str]:
    keys: dict[str, str] = {}
    in_group = False
    for line in text.splitlines():
        if line.startswith("["):
            in_group = line.strip() == "[Desktop Entry]"
        elif in_group and "=" in line and not line.lstrip().startswith("#"):
            key, value = line.split("=", 1)
            keys[key.strip()] = value.strip()
    return keys


def read_control_fields(deb: pathlib.Path) -> dict[str, str]:
    return parse_control_fields(require_output(run_command("dpkg-deb", "--field", str(deb))))


def list_deb_entries(deb: pathlib.Path) -> dict[str, DebEntry]:
    return parse_deb_listing(require_output(run_command("dpkg-deb", "--contents", str(deb))))


def read_deb_file(deb: pathlib.Path, path: str) -> str:
    result = subprocess.run(["dpkg-deb", "--fsys-tarfile", str(deb)], capture_output=True, check=False)
    if result.returncode != 0:
        raise DebCheckError(f"dpkg-deb --fsys-tarfile {deb} exited {result.returncode}")
    with tarfile.open(fileobj=io.BytesIO(result.stdout), mode="r:") as archive:
        for info in archive.getmembers():
            if info.name.removeprefix("./") == path and info.isreg():
                extracted = archive.extractfile(info)
                if extracted is None:
                    break
                return extracted.read().decode("utf-8")
    raise DebCheckError(f"{deb.name} has no regular file /{path}")


def get_package_status(package: str) -> str:
    result = run_command("dpkg-query", "--show", "--showformat=${db:Status-Status}", package)
    if result.returncode == 1 and "no packages found" in result.stderr:
        return "not-installed"
    return require_output(result).strip()


def get_package_owner(path: str) -> str:
    owner, separator, _ = require_output(run_command("dpkg-query", "--search", f"/{path}")).partition(":")
    if not separator:
        raise DebCheckError(f"no owner listed for /{path}")
    return owner.strip()


class ParseTests(unittest.TestCase):
    def test_a_listing_names_files_links_and_directories_without_a_leading_dot(self) -> None:
        listing = "\n".join(
            [
                "drwxr-xr-x 0/0               0 2026-11-18 10:00 ./usr/bin/",
                "-rwxr-xr-x 0/0         8275816 2026-11-18 10:00 usr/bin/jotra",
                "lrwxrwxrwx 0/0               0 2026-11-18 10:00 usr/bin/writ-tauri -> jotra-tauri",
                "-rw-r--r-- root/root       310 2026-11-18 10:00 ./usr/share/applications/My App.desktop",
            ]
        )
        self.assertEqual(
            parse_deb_listing(listing),
            {
                "usr/bin": DebEntry("directory"),
                COMMAND_PATH: DebEntry("file"),
                LEGACY_LAUNCHER_PATH: DebEntry("link", "jotra-tauri"),
                "usr/share/applications/My App.desktop": DebEntry("file"),
            },
        )

    def test_a_listing_line_it_cannot_read_is_refused(self) -> None:
        with self.assertRaises(DebCheckError):
            parse_deb_listing("lrwxrwxrwx 0/0 0 2026-11-18 10:00 usr/bin/writ-tauri")
        with self.assertRaises(DebCheckError):
            parse_deb_listing("usr/bin/jotra")

    def test_control_fields_keep_continuation_lines(self) -> None:
        fields = parse_control_fields("Package: jotra\nConflicts: writ\nDescription: Text\n more text\n")
        self.assertEqual(fields["Package"], PACKAGE)
        self.assertEqual(fields["Conflicts"], LEGACY_PACKAGE)
        self.assertEqual(fields["Description"], "Text\nmore text")

    def test_desktop_keys_come_from_the_desktop_entry_group_only(self) -> None:
        text = "[Desktop Entry]\nExec=jotra-tauri %F\n# NoDisplay=false\n\n[Desktop Action new]\nExec=other\n"
        self.assertEqual(parse_desktop_entry(text), {"Exec": "jotra-tauri %F"})


class NewDebContentsTests(unittest.TestCase):
    deb: pathlib.Path
    entries: dict[str, DebEntry]

    @classmethod
    def setUpClass(cls) -> None:
        deb = get_deb_path(TO_VARIABLE)
        if deb is None:
            raise unittest.SkipTest(f"{TO_VARIABLE} is not set")
        if shutil.which("dpkg-deb") is None:
            raise unittest.SkipTest("dpkg-deb is not installed")
        cls.deb = deb
        cls.entries = list_deb_entries(deb)

    def test_the_relationship_fields_name_the_legacy_package(self) -> None:
        fields = read_control_fields(self.deb)
        self.assertEqual(fields.get("Package"), PACKAGE)
        for field in RELATIONSHIP_FIELDS:
            with self.subTest(field=field):
                self.assertEqual(fields.get(field), LEGACY_PACKAGE)

    def test_the_legacy_launcher_path_is_a_link_to_the_launcher(self) -> None:
        self.assertEqual(self.entries.get(LAUNCHER_PATH), DebEntry("file"))
        self.assertEqual(self.entries.get(LEGACY_LAUNCHER_PATH), DebEntry("link", pathlib.PurePath(LAUNCHER_PATH).name))

    def test_both_commands_are_files(self) -> None:
        for path in (COMMAND_PATH, LEGACY_COMMAND_PATH):
            with self.subTest(path=path):
                self.assertEqual(self.entries.get(path), DebEntry("file"))

    def test_the_legacy_desktop_entry_is_hidden_and_opens_the_same_files(self) -> None:
        current = parse_desktop_entry(read_deb_file(self.deb, DESKTOP_PATH))
        legacy = parse_desktop_entry(read_deb_file(self.deb, LEGACY_DESKTOP_PATH))
        self.assertEqual(legacy.get("NoDisplay"), "true")
        self.assertNotEqual(current.get("NoDisplay"), "true")
        for key in ("Exec", "MimeType"):
            with self.subTest(key=key):
                self.assertTrue(current.get(key), f"{DESKTOP_PATH} has no {key}")
                self.assertEqual(legacy.get(key), current.get(key))


class UpgradeTests(unittest.TestCase):
    version: str
    upgrade: subprocess.CompletedProcess[str]

    @classmethod
    def setUpClass(cls) -> None:
        previous = get_deb_path(FROM_VARIABLE)
        current = get_deb_path(TO_VARIABLE)
        if previous is None or current is None:
            raise unittest.SkipTest(f"{FROM_VARIABLE} and {TO_VARIABLE} are not both set")
        if shutil.which("dpkg") is None or shutil.which("sudo") is None:
            raise unittest.SkipTest("dpkg and sudo are required")
        previous_package = read_control_fields(previous).get("Package")
        if previous_package != LEGACY_PACKAGE:
            raise DebCheckError(f"{previous.name} is package {previous_package}, not {LEGACY_PACKAGE}")
        cls.version = read_control_fields(current)["Version"]
        require_output(run_command("sudo", "apt-get", "update"))
        require_output(run_command("sudo", "apt-get", "install", "--yes", str(previous)))
        status = get_package_status(LEGACY_PACKAGE)
        if status != "installed":
            raise DebCheckError(f"{previous.name} left {LEGACY_PACKAGE} {status}")
        cls.upgrade = run_command("sudo", "dpkg", "--install", str(current))

    def test_dpkg_installs_the_new_deb_over_the_legacy_package(self) -> None:
        self.assertEqual(self.upgrade.returncode, 0, f"{self.upgrade.stdout}\n{self.upgrade.stderr}")
        self.assertEqual(get_package_status(PACKAGE), "installed")

    def test_the_legacy_package_is_gone(self) -> None:
        self.assertIn(get_package_status(LEGACY_PACKAGE), GONE_STATUSES)

    def test_both_commands_run_the_new_version(self) -> None:
        for path in (COMMAND_PATH, LEGACY_COMMAND_PATH):
            with self.subTest(path=path):
                command = pathlib.PurePath(path).name
                self.assertEqual(shutil.which(command), f"/{path}")
                result = run_command(command, "--version")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(self.version, result.stdout)

    def test_the_legacy_launcher_path_resolves_to_the_launcher(self) -> None:
        self.assertEqual(os.path.realpath(f"/{LEGACY_LAUNCHER_PATH}"), f"/{LAUNCHER_PATH}")
        self.assertTrue(os.access(f"/{LAUNCHER_PATH}", os.X_OK))

    def test_the_new_package_owns_every_legacy_path(self) -> None:
        for path in (LEGACY_COMMAND_PATH, LEGACY_LAUNCHER_PATH, LEGACY_DESKTOP_PATH):
            with self.subTest(path=path):
                self.assertEqual(get_package_owner(path), PACKAGE)


def main() -> int:
    suite = unittest.defaultTestLoader.loadTestsFromModule(sys.modules[__name__])
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    for test, reason in result.skipped:
        print(f"skipped {test}: {reason}", file=sys.stderr)
    return 0 if result.wasSuccessful() and not result.skipped else 1


if __name__ == "__main__":
    sys.exit(main())
