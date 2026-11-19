"""Install the previous release's MSI, then the new MSI over it with msiexec.

The release workflow runs this as an administrator on a disposable Windows
runner:
    MSI_UPGRADE_FROM=<previous .msi> MSI_UPGRADE_TO=<new .msi> \\
    MSI_UPGRADE_VERSION=<new version> [MSI_UPGRADE_INSTALL_DIR=<folder>] \\
        python .github/scripts/test_msi_upgrade_legacy.py

MSI_UPGRADE_INSTALL_DIR installs the previous release into that folder instead
of its default one. MSI_UPGRADE_LOG_DIR keeps the msiexec logs there.
MSI_UPGRADE_REQUIRE_WINGET_CORRELATION=1 requires winget to list the install
under the package id, which needs a published manifest that names the upgrade
code. Without the first three variables the upgrade tests are skipped. Run as a
script, a skipped test fails the run.
"""

from __future__ import annotations

import dataclasses
import json
import ntpath
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from collections.abc import Callable

from check_process import ImmediateTextResult, ProcessDeadlineError, prepare_output_streams, run_bounded, start_process

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
PRODUCT = "Jotra"
LEGACY_PRODUCT = "Writ"
MANUFACTURER = "Ibrahem ID"
UPGRADE_CODE = "{AB97CC4D-EEA3-53EE-90D8-3C2767967676}"
INSTALL_DIR_KEY = rf"Software\{MANUFACTURER}\{PRODUCT}"
LEGACY_INSTALL_DIR_KEY = rf"Software\{MANUFACTURER}\{LEGACY_PRODUCT}"
UNINSTALL_KEY = r"Software\Microsoft\Windows\CurrentVersion\Uninstall"
RUN_KEY = r"Software\Microsoft\Windows\CurrentVersion\Run"
STARTUP_APPROVED_KEY = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
MACHINE_ENVIRONMENT_KEY = r"System\CurrentControlSet\Control\Session Manager\Environment"
USER_ENVIRONMENT_KEY = "Environment"
CLASSES_KEY = r"Software\Classes"
PROTOCOLS = ("jotra", "writ")
COMMANDS = ("jotra", "writ")
LAUNCHER = "jotra-tauri.exe"
LEGACY_LAUNCHER = "writ-tauri.exe"
SHORTCUT = f"{PRODUCT}.lnk"
LEGACY_SHORTCUT = f"{LEGACY_PRODUCT}.lnk"
WINGET_PACKAGE = "ibrahemid.Writ"
SEEDED_APPROVAL = bytes([0x03, 0, 0, 0]) + (133_000_000_000_000_000).to_bytes(8, "little")
FROM_VARIABLE = "MSI_UPGRADE_FROM"
TO_VARIABLE = "MSI_UPGRADE_TO"
VERSION_VARIABLE = "MSI_UPGRADE_VERSION"
INSTALL_DIR_VARIABLE = "MSI_UPGRADE_INSTALL_DIR"
LOG_DIR_VARIABLE = "MSI_UPGRADE_LOG_DIR"
PROGIDS_VARIABLE = "MSI_UPGRADE_PROGIDS"
WINGET_VARIABLE = "MSI_UPGRADE_REQUIRE_WINGET_CORRELATION"
DEFAULT_PROGIDS = REPO_ROOT / "src-tauri/wix/legacy-progids.wxs"
MSIEXEC_SUCCESS = {0, 3010}
ERROR_NO_MORE_ITEMS = 259
LAUNCH_TIMEOUT = 90.0
COMMAND_TIMEOUT = 120.0
MSIEXEC_TIMEOUT = 300.0
LEGACY_LAUNCHER_TIMEOUT = 60.0
STOP_TIMEOUT = 30.0
OPEN_COMMAND = re.compile(r'"([^"]+)" "%1"')
SEMVER_IDENTIFIER = r"(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)"
SEMVER = re.compile(
    r"(?P<major>0|[1-9]\d*)\.(?P<minor>0|[1-9]\d*)\.(?P<patch>0|[1-9]\d*)"
    rf"(?:-(?P<pre>{SEMVER_IDENTIFIER}(?:\.{SEMVER_IDENTIFIER})*))?"
    r"(?:\+(?P<build>[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?",
    re.ASCII,
)
MSI_FOURTH_FIELD_MAX = 65535


class MsiCheckError(RuntimeError):
    pass


@dataclasses.dataclass(frozen=True)
class UninstallEntry:
    location: str
    key: str
    name: str
    version: str
    publisher: str


@dataclasses.dataclass(frozen=True)
class WingetRow:
    name: str
    package_id: str
    version: str


def get_msi_path(variable: str) -> pathlib.Path | None:
    value = os.environ.get(variable, "")
    if not value:
        return None
    path = pathlib.Path(value).resolve()
    if not path.is_file():
        raise MsiCheckError(f"{variable} names {value}, which is not a file")
    return path


def run_command(*args: str, env: dict[str, str] | None = None, timeout: float = COMMAND_TIMEOUT) -> subprocess.CompletedProcess[str]:
    return run_bounded(args, timeout=timeout, env=env)


def require_output(result: subprocess.CompletedProcess[str]) -> str:
    if result.returncode != 0:
        command = " ".join(str(arg) for arg in result.args)
        raise MsiCheckError(f"{command} exited {result.returncode}: {result.stderr.strip()}")
    return result.stdout


def get_msi_product_version(version: str) -> str:
    match = SEMVER.fullmatch(version)
    if match is None:
        raise MsiCheckError(f"{version!r} is not a semantic version")
    core = f"{match['major']}.{match['minor']}.{match['patch']}"
    for label, field in (("build metadata", match["build"]), ("pre-release identifier", match["pre"])):
        if field is None:
            continue
        if not field.isdigit() or int(field) > MSI_FOURTH_FIELD_MAX:
            raise MsiCheckError(f"the {label} of {version} must be a number no greater than {MSI_FOURTH_FIELD_MAX} for an MSI")
        return f"{core}.{field}"
    return version


def normalize_dir(path: str) -> str:
    return ntpath.normcase(ntpath.normpath(path.strip().strip('"')))


def parse_progid_extensions(text: str) -> list[str]:
    return re.findall(r'Key="Software\\Classes\\Writ\.([^"\\]+)"', text)


def open_command(install_dir: str) -> str:
    return f'"{pathlib.PureWindowsPath(install_dir, LAUNCHER)}" "%1"'


def get_long_path(path: str) -> str:
    import ctypes
    from ctypes import wintypes

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    function = kernel32.GetLongPathNameW
    function.argtypes = [wintypes.LPCWSTR, wintypes.LPWSTR, wintypes.DWORD]
    function.restype = wintypes.DWORD
    size = function(path, None, 0)
    buffer = ctypes.create_unicode_buffer(max(size, 1))
    if size == 0 or function(path, buffer, size) == 0:
        raise MsiCheckError(f"GetLongPathNameW({path}) failed with Windows error {ctypes.get_last_error()}")
    return buffer.value


def get_open_command_target(command: object, resolve: Callable[[str], str] = get_long_path) -> str:
    match = OPEN_COMMAND.fullmatch(command) if isinstance(command, str) else None
    if match is None:
        raise MsiCheckError(f'open command {command!r} is not "<program>" "%1"')
    return resolve(match.group(1))


def expand_path_entries(machine: str, user: str, expand=os.path.expandvars) -> list[str]:
    return [expand(entry) for entry in f"{machine};{user}".split(";") if entry.strip()]


def clean_console_line(line: str) -> str:
    line = line.rsplit("\r", 1)[-1]
    while "\b" in line:
        line = re.sub(r"[^\b]?\b", "", line, count=1)
    return line.rstrip()


def parse_winget_table(text: str) -> list[WingetRow]:
    lines = [clean_console_line(line) for line in text.splitlines()]
    header_index = next((i for i, line in enumerate(lines) if re.search(r"\bName\s+Id\s+Version\b", line)), None)
    if header_index is None:
        return []
    header = lines[header_index]
    starts = [header.index(column) for column in ("Name", "Id", "Version")]
    following = re.search(r"\S", header[starts[2] + len("Version") :])
    ends = [starts[1], starts[2], starts[2] + len("Version") + following.start() if following else None]
    rows = []
    for line in lines[header_index + 1 :]:
        if not line.strip() or set(line.strip()) == {"-"}:
            continue
        if re.match(r"^\d+ (upgrades|packages?) ", line.strip()):
            continue
        cells = [line[start:end].strip() for start, end in zip(starts, ends)]
        rows.append(WingetRow(*cells))
    return rows


def parse_process_list(text: str) -> list[dict[str, object]]:
    if not text.strip():
        return []
    parsed = json.loads(text)
    return parsed if isinstance(parsed, list) else [parsed]


def read_log_tail(path: pathlib.Path, lines: int = 60) -> str:
    if not path.is_file():
        return f"(no log at {path})"
    raw = path.read_bytes()
    text = raw.decode("utf-16") if raw[:2] == b"\xff\xfe" else raw.decode("utf-8", errors="replace")
    return "\n".join(text.splitlines()[-lines:])


def registry_root(name: str) -> int:
    import winreg

    return {"HKLM": winreg.HKEY_LOCAL_MACHINE, "HKCU": winreg.HKEY_CURRENT_USER}[name]


def read_registry_value(root: str, key: str, name: str, view: int | None = None) -> object | None:
    import winreg

    access = winreg.KEY_READ | (winreg.KEY_WOW64_64KEY if view is None else view)
    try:
        with winreg.OpenKey(registry_root(root), key, 0, access) as handle:
            value, _ = winreg.QueryValueEx(handle, name)
            return value
    except FileNotFoundError:
        return None


def write_registry_value(root: str, key: str, name: str, kind: int, value: object) -> None:
    import winreg

    with winreg.CreateKeyEx(registry_root(root), key, 0, winreg.KEY_SET_VALUE) as handle:
        winreg.SetValueEx(handle, name, 0, kind, value)


def list_uninstall_entries() -> list[UninstallEntry]:
    import winreg

    entries = []
    views = (("HKLM", winreg.KEY_WOW64_64KEY), ("HKLM", winreg.KEY_WOW64_32KEY), ("HKCU", winreg.KEY_WOW64_64KEY))
    for root, view in views:
        try:
            handle = winreg.OpenKey(registry_root(root), UNINSTALL_KEY, 0, winreg.KEY_READ | view)
        except FileNotFoundError:
            continue
        with handle:
            index = 0
            while True:
                try:
                    subkey = winreg.EnumKey(handle, index)
                except OSError:
                    break
                index += 1
                location = rf"{UNINSTALL_KEY}\{subkey}"
                name = read_registry_value(root, location, "DisplayName", view)
                if name in (PRODUCT, LEGACY_PRODUCT):
                    version = read_registry_value(root, location, "DisplayVersion", view) or ""
                    publisher = read_registry_value(root, location, "Publisher", view) or ""
                    view_name = "32-bit" if view == winreg.KEY_WOW64_32KEY else "64-bit"
                    entries.append(UninstallEntry(f"{root} {view_name}", subkey, str(name), str(version), str(publisher)))
    return entries


def get_related_products(upgrade_code: str) -> list[str]:
    import ctypes

    msi = ctypes.windll.msi
    products = []
    index = 0
    while True:
        buffer = ctypes.create_unicode_buffer(39)
        status = msi.MsiEnumRelatedProductsW(upgrade_code, 0, index, buffer)
        if status == ERROR_NO_MORE_ITEMS:
            return products
        if status != 0:
            raise MsiCheckError(f"MsiEnumRelatedProductsW({upgrade_code}, index {index}) returned {status}")
        products.append(buffer.value)
        index += 1


def install_msi(msi: pathlib.Path, log: pathlib.Path, install_dir: str | None = None) -> None:
    command = f'msiexec /i "{msi}" /qn /norestart /l*v "{log}"'
    if install_dir:
        command += f' INSTALLDIR="{install_dir}"'
    try:
        result = run_bounded(command, timeout=MSIEXEC_TIMEOUT)
    except ProcessDeadlineError as error:
        raise MsiCheckError(f"{error}\nlog tail:\n{read_log_tail(log)}") from error
    if result.returncode not in MSIEXEC_SUCCESS:
        raise MsiCheckError(f"{command} exited {result.returncode}; log tail:\n{read_log_tail(log)}")
    print(f"installed {msi.name} (msiexec exit {result.returncode}, log {log})", flush=True)


def list_processes(image: str) -> list[dict[str, object]]:
    script = (
        f"Get-CimInstance Win32_Process -Filter \"Name='{image}'\" | "
        "Select-Object ProcessId,ExecutablePath,CommandLine | ConvertTo-Json -Compress"
    )
    return parse_process_list(require_output(run_command("powershell", "-NoProfile", "-Command", script)))


def stop_processes(image: str) -> None:
    run_command("taskkill", "/F", "/T", "/IM", image)
    deadline = time.monotonic() + STOP_TIMEOUT
    remaining = list_processes(image)
    while remaining and time.monotonic() < deadline:
        time.sleep(1)
        remaining = list_processes(image)
    if remaining:
        raise MsiCheckError(f"{image} still runs {STOP_TIMEOUT:.0f}s after taskkill: {remaining}")


def read_shortcut_target(path: pathlib.Path) -> str:
    script = "(New-Object -ComObject WScript.Shell).CreateShortcut($env:SHORTCUT_PATH).TargetPath"
    env = {**os.environ, "SHORTCUT_PATH": str(path)}
    return require_output(run_command("powershell", "-NoProfile", "-Command", script, env=env)).strip()


def get_start_menu_dir() -> pathlib.Path:
    return pathlib.Path(os.environ["ProgramData"], "Microsoft/Windows/Start Menu/Programs")


def get_desktop_dirs() -> list[pathlib.Path]:
    return [pathlib.Path(os.environ["PUBLIC"], "Desktop"), pathlib.Path(os.environ["USERPROFILE"], "Desktop")]


def find_desktop_shortcuts(name: str) -> list[pathlib.Path]:
    return [directory / name for directory in get_desktop_dirs() if (directory / name).exists()]


class ParseTests(unittest.TestCase):
    def test_progid_extensions_come_from_the_legacy_keys(self) -> None:
        text = (
            '<RegistryKey Root="HKLM" Key="Software\\Classes\\Writ.txt">\n'
            '<RegistryKey Key="shell\\open">\n'
            '<RegistryKey Root="HKLM" Key="Software\\Classes\\Writ.code-workspace">\n'
            '<RegistryKey Root="HKLM" Key="Software\\Classes\\jotra">\n'
        )
        self.assertEqual(parse_progid_extensions(text), ["txt", "code-workspace"])

    def test_the_repository_fragment_lists_progids(self) -> None:
        extensions = parse_progid_extensions(DEFAULT_PROGIDS.read_text())
        self.assertIn("md", extensions)
        self.assertEqual(len(extensions), len(set(extensions)))

    def test_the_open_command_quotes_the_launcher_and_the_argument(self) -> None:
        self.assertEqual(
            open_command("C:\\Apps\\Writ Custom\\"), '"C:\\Apps\\Writ Custom\\jotra-tauri.exe" "%1"'
        )

    def test_a_protocol_command_names_its_program_through_the_resolved_path(self) -> None:
        long_paths = {"C:\\PROGRA~1\\Writ\\JOTRA-~1.EXE": "C:\\Program Files\\Writ\\jotra-tauri.exe"}
        target = get_open_command_target('"C:\\PROGRA~1\\Writ\\JOTRA-~1.EXE" "%1"', long_paths.__getitem__)
        self.assertEqual(normalize_dir(target), normalize_dir("C:\\Program Files\\Writ\\jotra-tauri.exe"))

    def test_a_protocol_command_without_the_quoted_argument_is_refused(self) -> None:
        for command in ('"C:\\Writ\\jotra-tauri.exe"', 'C:\\Writ\\jotra-tauri.exe "%1"', '"C:\\Writ\\jotra-tauri.exe" %1', None):
            with self.subTest(command=command), self.assertRaises(MsiCheckError):
                get_open_command_target(command, str)

    def test_path_entries_expand_and_skip_blanks(self) -> None:
        entries = expand_path_entries(
            "%SystemRoot%\\system32;;C:\\Program Files\\Writ\\", "C:\\Users\\a\\bin;", lambda text: text.replace("%SystemRoot%", "C:\\Windows")
        )
        self.assertEqual(entries, ["C:\\Windows\\system32", "C:\\Program Files\\Writ\\", "C:\\Users\\a\\bin"])

    def test_a_winget_table_is_read_past_its_spinner(self) -> None:
        arp = "ARP\\Machine\\X64\\{0A1B2C3D-0000-0000-0000-000000000000}"
        text = (
            "   - \r   \\ \r   | \r"
            f"{'Name':<7}{'Id':<57}{'Version':<8}Source\r\n"
            f"{'-' * 78}\r\n"
            f"{'Jotra':<7}{arp:<57}1.0.2\r\n"
            f"{'Writ':<7}{'ibrahemid.Writ':<57}{'1.0.1':<8}winget\r\n"
        )
        self.assertEqual(
            parse_winget_table(text),
            [WingetRow("Jotra", arp, "1.0.2"), WingetRow("Writ", "ibrahemid.Writ", "1.0.1")],
        )

    def test_a_winget_reply_without_a_table_has_no_rows(self) -> None:
        self.assertEqual(parse_winget_table("No installed package found matching input criteria.\n"), [])

    def test_a_process_list_reads_one_object_or_many(self) -> None:
        one = '{"ProcessId":1,"ExecutablePath":"C:\\\\a.exe","CommandLine":"a"}'
        self.assertEqual(len(parse_process_list(one)), 1)
        self.assertEqual(len(parse_process_list(f"[{one},{one}]")), 2)
        self.assertEqual(parse_process_list(""), [])

    def test_a_folder_compares_without_case_quotes_or_trailing_separator(self) -> None:
        self.assertEqual(normalize_dir('"C:\\Program Files\\Writ\\"'), normalize_dir("c:\\program files\\writ"))

    def test_the_product_version_carries_a_candidate_number_as_its_fourth_field(self) -> None:
        cases = {
            "1.1.0": "1.1.0",
            "1.1.0-1": "1.1.0.1",
            "1.0.0-2": "1.0.0.2",
            "1.1.0+7": "1.1.0.7",
            "1.1.0-1+7": "1.1.0.7",
            "1.1.0-65535": "1.1.0.65535",
        }
        for version, expected in cases.items():
            with self.subTest(version=version):
                self.assertEqual(get_msi_product_version(version), expected)

    def test_a_version_the_msi_bundler_refuses_has_no_product_version(self) -> None:
        for version in ("1.1.0-rc.1", "1.1.0-1.2", "1.1.0-65536", "1.1.0+build.1", "1.1.0-01", "1.1", "v1.1.0", "1.1.0-"):
            with self.subTest(version=version), self.assertRaises(MsiCheckError):
                get_msi_product_version(version)

    def test_the_seeded_approval_reads_as_disabled(self) -> None:
        self.assertEqual(len(SEEDED_APPROVAL), 12)
        self.assertEqual(SEEDED_APPROVAL[0], 0x03)


class UpgradeTests(unittest.TestCase):
    version: str
    product_version: str
    install_dir: str
    legacy_command: str
    legacy_desktop_shortcuts: list[pathlib.Path]

    @classmethod
    def setUpClass(cls) -> None:
        previous = get_msi_path(FROM_VARIABLE)
        current = get_msi_path(TO_VARIABLE)
        version = os.environ.get(VERSION_VARIABLE, "")
        if previous is None or current is None or not version:
            raise unittest.SkipTest(f"{FROM_VARIABLE}, {TO_VARIABLE} and {VERSION_VARIABLE} are not all set")
        if sys.platform != "win32":
            raise unittest.SkipTest("the MSI upgrade runs on Windows only")
        import winreg

        cls.version = version
        cls.product_version = get_msi_product_version(version)
        log_dir = pathlib.Path(os.environ.get(LOG_DIR_VARIABLE) or tempfile.mkdtemp(prefix="msi-upgrade-"))
        log_dir.mkdir(parents=True, exist_ok=True)
        requested = os.environ.get(INSTALL_DIR_VARIABLE, "") or None

        install_msi(previous, log_dir / "previous.log", requested)
        old_dir = read_registry_value("HKCU", LEGACY_INSTALL_DIR_KEY, "InstallDir")
        if not isinstance(old_dir, str) or not old_dir:
            raise MsiCheckError(f"{previous.name} left no InstallDir under HKCU\\{LEGACY_INSTALL_DIR_KEY}")
        if requested and normalize_dir(old_dir) != normalize_dir(requested):
            raise MsiCheckError(f"{previous.name} installed into {old_dir}, not the requested {requested}")
        if not pathlib.Path(old_dir, LEGACY_LAUNCHER).is_file():
            raise MsiCheckError(f"{previous.name} left no {LEGACY_LAUNCHER} in {old_dir}")
        cls.install_dir = old_dir
        cls.legacy_desktop_shortcuts = find_desktop_shortcuts(LEGACY_SHORTCUT)
        print(f"previous install: {old_dir}; desktop shortcuts: {cls.legacy_desktop_shortcuts}", flush=True)

        legacy_command = f'"{pathlib.PureWindowsPath(old_dir, LEGACY_LAUNCHER)}" --login'
        write_registry_value("HKCU", RUN_KEY, LEGACY_PRODUCT, winreg.REG_SZ, legacy_command)
        write_registry_value("HKCU", STARTUP_APPROVED_KEY, LEGACY_PRODUCT, winreg.REG_BINARY, SEEDED_APPROVAL)
        cls.legacy_command = legacy_command

        install_msi(current, log_dir / "current.log")

    def tearDown(self) -> None:
        stop_processes(LAUNCHER)

    def test_apps_and_features_lists_one_jotra_entry_under_the_upgrade_code(self) -> None:
        entries = list_uninstall_entries()
        self.assertEqual([entry.name for entry in entries], [PRODUCT], f"Apps and Features entries: {entries}")
        [entry] = entries
        self.assertEqual(entry.version, self.product_version, f"Apps and Features entry: {entry}")
        self.assertEqual(entry.publisher, MANUFACTURER, f"Apps and Features entry: {entry}")
        related = get_related_products(UPGRADE_CODE)
        self.assertEqual(
            [code.upper() for code in related],
            [entry.key.upper()],
            f"products under upgrade code {UPGRADE_CODE}: {related}; Apps and Features key: {entry.key}",
        )

    def test_the_install_folder_is_kept(self) -> None:
        recorded = read_registry_value("HKCU", INSTALL_DIR_KEY, "InstallDir")
        self.assertIsInstance(recorded, str, f"HKCU\\{INSTALL_DIR_KEY} InstallDir: {recorded!r}")
        self.assertEqual(
            normalize_dir(str(recorded)),
            normalize_dir(self.install_dir),
            f"InstallDir after the upgrade is {recorded}, the previous install used {self.install_dir}",
        )
        for name in (LAUNCHER, LEGACY_LAUNCHER, "jotra.exe", "writ.exe"):
            with self.subTest(file=name):
                self.assertTrue(pathlib.Path(self.install_dir, name).is_file(), f"{name} is missing from {self.install_dir}")
        default = pathlib.Path(os.environ["ProgramW6432"], PRODUCT)
        self.assertFalse(default.exists(), f"the upgrade also created {default}")

    def test_both_commands_resolve_in_a_new_shell(self) -> None:
        machine = read_registry_value("HKLM", MACHINE_ENVIRONMENT_KEY, "Path") or ""
        user = read_registry_value("HKCU", USER_ENVIRONMENT_KEY, "Path") or ""
        entries = expand_path_entries(str(machine), str(user))
        self.assertIn(
            normalize_dir(self.install_dir),
            [normalize_dir(entry) for entry in entries],
            f"PATH in the registry after the upgrade: {entries}",
        )
        env = {**os.environ, "PATH": ";".join(entries)}
        for command in COMMANDS:
            with self.subTest(command=command):
                found = run_command("where", command, env=env)
                first = found.stdout.splitlines()[0] if found.stdout.strip() else ""
                self.assertEqual(
                    normalize_dir(first),
                    normalize_dir(str(pathlib.PureWindowsPath(self.install_dir, f"{command}.exe"))),
                    f"where {command} in a new shell: {found.stdout.strip() or found.stderr.strip()}",
                )
                result = run_command("cmd", "/d", "/c", command, "--version", env=env)
                self.assertEqual(result.returncode, 0, f"{command} --version: {result.stdout}{result.stderr}")
                self.assertIn(self.version, result.stdout, f"{command} --version printed {result.stdout.strip()!r}")

    def test_the_start_menu_shortcut_carries_the_new_name(self) -> None:
        programs = get_start_menu_dir()
        shortcut = programs / PRODUCT / SHORTCUT
        self.assertTrue(shortcut.is_file(), f"no {shortcut}; Start Menu folders: {sorted(p.name for p in programs.iterdir())}")
        target = read_shortcut_target(shortcut)
        self.assertEqual(
            normalize_dir(target),
            normalize_dir(str(pathlib.PureWindowsPath(self.install_dir, LAUNCHER))),
            f"{shortcut} points at {target}",
        )
        self.assertFalse((programs / LEGACY_PRODUCT).exists(), f"{programs / LEGACY_PRODUCT} is still there")

    def test_the_desktop_shortcut_carries_the_new_name(self) -> None:
        self.assertTrue(self.legacy_desktop_shortcuts, "the previous release left no desktop shortcut to carry over")
        self.assertEqual(find_desktop_shortcuts(LEGACY_SHORTCUT), [], "the old desktop shortcut is still there")
        current = find_desktop_shortcuts(SHORTCUT)
        self.assertTrue(current, f"no {SHORTCUT} on {[str(d) for d in get_desktop_dirs()]} after the upgrade")
        target = read_shortcut_target(current[0])
        self.assertEqual(
            normalize_dir(target),
            normalize_dir(str(pathlib.PureWindowsPath(self.install_dir, LAUNCHER))),
            f"{current[0]} points at {target}",
        )

    def test_the_login_item_moves_to_the_new_name_and_keeps_its_state(self) -> None:
        started = start_process([str(pathlib.Path(self.install_dir, LAUNCHER))])
        try:
            deadline = time.monotonic() + LAUNCH_TIMEOUT
            while time.monotonic() < deadline:
                if read_registry_value("HKCU", RUN_KEY, LEGACY_PRODUCT) is None:
                    break
                time.sleep(1)
        finally:
            started.stop()
        current = read_registry_value("HKCU", RUN_KEY, PRODUCT)
        legacy = read_registry_value("HKCU", RUN_KEY, LEGACY_PRODUCT)
        self.assertIsNone(legacy, f"Run\\{LEGACY_PRODUCT} is still {legacy!r} {LAUNCH_TIMEOUT:.0f}s after launch")
        self.assertEqual(current, self.legacy_command, f"Run\\{PRODUCT} after launch: {current!r}")
        approval = read_registry_value("HKCU", STARTUP_APPROVED_KEY, PRODUCT)
        self.assertEqual(approval, SEEDED_APPROVAL, f"StartupApproved\\Run\\{PRODUCT}: {approval!r}")
        legacy_approval = read_registry_value("HKCU", STARTUP_APPROVED_KEY, LEGACY_PRODUCT)
        self.assertIsNone(legacy_approval, f"StartupApproved\\Run\\{LEGACY_PRODUCT} is still {legacy_approval!r}")

    def test_both_url_protocols_open_the_new_launcher(self) -> None:
        expected = normalize_dir(str(pathlib.PureWindowsPath(self.install_dir, LAUNCHER)))
        for protocol in PROTOCOLS:
            with self.subTest(protocol=protocol):
                key = rf"{CLASSES_KEY}\{protocol}"
                marker = read_registry_value("HKLM", key, "URL Protocol")
                self.assertEqual(marker, "", f"HKLM\\{key} URL Protocol: {marker!r}")
                command = read_registry_value("HKLM", rf"{key}\shell\open\command", "")
                target = get_open_command_target(command)
                self.assertEqual(
                    normalize_dir(target), expected, f"HKLM\\{key}\\shell\\open\\command is {command!r}, which opens {target}"
                )

    def test_the_legacy_progids_open_the_new_launcher(self) -> None:
        fragment = pathlib.Path(os.environ.get(PROGIDS_VARIABLE) or DEFAULT_PROGIDS)
        extensions = parse_progid_extensions(fragment.read_text())
        self.assertTrue(extensions, f"{fragment} lists no Writ.<ext> ProgIds")
        expected = open_command(self.install_dir)
        wrong = {}
        for extension in extensions:
            key = rf"{CLASSES_KEY}\Writ.{extension}\shell\open\command"
            command = read_registry_value("HKLM", key, "")
            if command != expected:
                wrong[f"Writ.{extension}"] = command
        self.assertEqual(wrong, {}, f"ProgIds whose open command is not {expected}")

    def test_the_legacy_launcher_starts_jotra_with_its_arguments(self) -> None:
        stop_processes(LAUNCHER)
        with tempfile.TemporaryDirectory(prefix="msi-upgrade-open-", ignore_cleanup_errors=True) as folder:
            document = pathlib.Path(folder, "opened by the legacy launcher.md")
            document.write_text("# check\n")
            try:
                result = run_command(
                    str(pathlib.Path(self.install_dir, LEGACY_LAUNCHER)), str(document), timeout=LEGACY_LAUNCHER_TIMEOUT
                )
                self.assertEqual(result.returncode, 0, f"{LEGACY_LAUNCHER} {document}: {result.stdout}{result.stderr}")
                expected = normalize_dir(str(pathlib.Path(self.install_dir, LAUNCHER)))
                deadline = time.monotonic() + LAUNCH_TIMEOUT
                started: list[dict[str, object]] = []
                while time.monotonic() < deadline:
                    started = [p for p in list_processes(LAUNCHER) if normalize_dir(str(p.get("ExecutablePath") or "")) == expected]
                    if started:
                        break
                    time.sleep(1)
                self.assertTrue(started, f"no {expected} started by {LEGACY_LAUNCHER} within {LAUNCH_TIMEOUT:.0f}s; running: {list_processes(LAUNCHER)}")
                self.assertTrue(
                    any(str(document) in str(p.get("CommandLine") or "") for p in started),
                    f"{LAUNCHER} started without {document}: {started}",
                )
            finally:
                stop_processes(LAUNCHER)

    def test_winget_lists_one_entry(self) -> None:
        winget = shutil.which("winget")
        if winget is None:
            print("winget is not on this runner; the winget listing is not checked", flush=True)
            return
        result = run_command(
            winget, "list", "--name", PRODUCT, "--accept-source-agreements", "--disable-interactivity", timeout=300
        )
        rows = parse_winget_table(result.stdout)
        print(f"winget list --name {PRODUCT} (exit {result.returncode}):\n{result.stdout}", flush=True)
        products = [row for row in rows if row.name in (PRODUCT, LEGACY_PRODUCT)]
        self.assertEqual([row.name for row in products], [PRODUCT], f"winget rows: {rows}")
        [row] = products
        if os.environ.get(WINGET_VARIABLE) == "1":
            self.assertEqual(row.package_id, WINGET_PACKAGE, f"winget lists the install as {row.package_id}")
        else:
            print(f"winget lists the install as {row.package_id}; {WINGET_VARIABLE} is not 1, so {WINGET_PACKAGE} is not required", flush=True)


def main() -> int:
    prepare_output_streams()
    suite = unittest.defaultTestLoader.loadTestsFromModule(sys.modules[__name__])
    result = unittest.TextTestRunner(verbosity=2, resultclass=ImmediateTextResult).run(suite)
    for test, reason in result.skipped:
        print(f"skipped {test}: {reason}", file=sys.stderr)
    return 0 if result.wasSuccessful() and not result.skipped else 1


if __name__ == "__main__":
    sys.exit(main())
