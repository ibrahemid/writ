"""Install the previous release into /Applications, replace it the way the updater does, then launch.

The release workflow runs this on a disposable macOS runner:
    MACOS_UPGRADE_FROM=<previous .app.tar.gz or .pkg> MACOS_UPGRADE_TO=<new .app.tar.gz> \\
    MACOS_UPGRADE_OWNER=<user|admin> MACOS_UPGRADE_VERSION=<new version> \\
        python3 .github/scripts/test_macos_upgrade_legacy.py

MACOS_UPGRADE_OWNER=user extracts the previous archive into /Applications as
the runner's user; admin installs the previous .pkg with installer, which
leaves the bundle owned by root. The replace follows tauri-plugin-updater
2.14.1, except that its administrator prompt is `sudo -n`. Without the four
variables the upgrade tests are skipped. Run as a script, a skipped test fails
the run.
"""

from __future__ import annotations

import ctypes
import dataclasses
import errno
import json
import os
import pathlib
import plistlib
import select
import signal
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import unittest

from check_process import ImmediateTextResult, run_bounded, stop_process_tree

APPLICATIONS = pathlib.Path("/Applications")
BUNDLE_NAME = "Jotra.app"
LEGACY_BUNDLE_NAME = "Writ.app"
BUNDLE = APPLICATIONS / BUNDLE_NAME
LEGACY_BUNDLE = APPLICATIONS / LEGACY_BUNDLE_NAME
IDENTIFIER = "com.writ.editor"
EXECUTABLE = "jotra-tauri"
LEGACY_EXECUTABLE = "writ-tauri"
LEGACY_COMMAND = "writ"
LEGACY_SERVER_NAME = "writ"
FROM_VARIABLE = "MACOS_UPGRADE_FROM"
TO_VARIABLE = "MACOS_UPGRADE_TO"
OWNER_VARIABLE = "MACOS_UPGRADE_OWNER"
VERSION_VARIABLE = "MACOS_UPGRADE_VERSION"
OWNERS = ("user", "admin")
RENAME_SWAP = 2
CURRENT_APP_PREFIX = "tauri_current_app"
UPDATED_APP_PREFIX = "tauri_updated_app"
SWAP_JXA = (
    "function run(argv) {\n"
    '  ObjC.import("stdio");\n'
    '  return $.renamex_np(argv[0], argv[1], 2) === 0 ? "swapped" : "failed";\n'
    "}"
)
LAUNCH_TIMEOUT = 120.0
STABLE_SECONDS = 5.0
MCP_TIMEOUT = 60.0
COMMAND_TIMEOUT = 120.0
INSTALL_TIMEOUT = 300.0
STOP_TIMEOUT = 30.0
KILL_TIMEOUT = 10.0
LOG_FOLDERS = (pathlib.Path.home() / ".jotra/logs", pathlib.Path.home() / ".writ/logs")


class MacosCheckError(RuntimeError):
    pass


@dataclasses.dataclass(frozen=True)
class AppProcess:
    pid: int
    path: str


def get_artifact_path(variable: str) -> pathlib.Path | None:
    value = os.environ.get(variable, "")
    if not value:
        return None
    path = pathlib.Path(value).resolve()
    if not path.is_file():
        raise MacosCheckError(f"{variable} names {value}, which is not a file")
    return path


def run_command(*args: str, timeout: float = COMMAND_TIMEOUT) -> subprocess.CompletedProcess[str]:
    return run_bounded(args, timeout=timeout)


def require_output(result: subprocess.CompletedProcess[str]) -> str:
    if result.returncode != 0:
        command = " ".join(str(arg) for arg in result.args)
        raise MacosCheckError(f"{command} exited {result.returncode}: {result.stderr.strip()}")
    return result.stdout


def sh_quote(text: str) -> str:
    return "'" + text.replace("'", "'\\''") + "'"


def privileged_swap_command(staged: pathlib.Path, target: pathlib.Path, backup: pathlib.Path) -> str:
    staged_q, target_q, backup_q = (sh_quote(str(path)) for path in (staged, target, backup))
    return (
        f"case \"$(/usr/bin/osascript -l JavaScript -e {sh_quote(SWAP_JXA)} {staged_q} {target_q} 2>/dev/null)\" in "
        f"swapped) rm -rf {staged_q};; "
        f"failed) mv -f {target_q} {backup_q} && {{ mv -f {staged_q} {target_q} || {{ mv -f {backup_q} {target_q}; exit 1; }}; }} "
        f"&& rm -rf {backup_q};; "
        "*) exit 1;; esac"
    )


def choose_temp_root(target: pathlib.Path, candidates: list[pathlib.Path], stat_path=os.stat) -> pathlib.Path:
    device = os.lstat(target).st_dev
    for candidate in candidates:
        try:
            if stat_path(candidate).st_dev == device:
                return candidate
        except OSError:
            continue
    return candidates[0]


def strip_first_component(name: str) -> str | None:
    parts = pathlib.PurePosixPath(name).parts[1:]
    return str(pathlib.PurePosixPath(*parts)) if parts else None


def extract_update(archive: pathlib.Path, staged: pathlib.Path) -> None:
    with tarfile.open(archive, "r:gz") as tar:
        members = []
        for member in tar.getmembers():
            stripped = strip_first_component(member.name)
            if stripped is None:
                continue
            member.name = stripped
            if member.islnk():
                member.linkname = strip_first_component(member.linkname) or member.linkname
            members.append(member)
        if hasattr(tarfile, "tar_filter"):
            tar.extractall(staged, members=members, filter="tar")
        else:
            tar.extractall(staged, members=members)
    os.chmod(staged, 0o755)


def rename_swap(staged: pathlib.Path, target: pathlib.Path) -> None:
    libc = ctypes.CDLL(None, use_errno=True)
    libc.renamex_np.argtypes = (ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint)
    if libc.renamex_np(os.fsencode(staged), os.fsencode(target), RENAME_SWAP) != 0:
        code = ctypes.get_errno()
        raise OSError(code, os.strerror(code), str(target))


def is_permission_error(error: OSError) -> bool:
    return isinstance(error, PermissionError) or error.errno in (errno.EACCES, errno.EPERM)


def replace_bundle(archive: pathlib.Path, target: pathlib.Path) -> str:
    root = choose_temp_root(target, [pathlib.Path(tempfile.gettempdir()), target.parent])
    staged = pathlib.Path(tempfile.mkdtemp(prefix=UPDATED_APP_PREFIX, dir=root))
    backup_dir = pathlib.Path(tempfile.mkdtemp(prefix=CURRENT_APP_PREFIX, dir=root))
    backup = backup_dir / "current_app"
    print(f"updater replace: staging {archive.name} in {staged}", flush=True)
    extract_update(archive, staged)
    try:
        rename_swap(staged, target)
        branch = "renamex_np swap"
        subprocess.run(["rm", "-rf", str(staged)], check=False)
    except OSError as error:
        if is_permission_error(error):
            command = privileged_swap_command(staged, target, backup)
            result = run_command("sudo", "-n", "/bin/sh", "-c", command, timeout=INSTALL_TIMEOUT)
            if result.returncode != 0:
                raise MacosCheckError(
                    f"the privileged swap exited {result.returncode}: {result.stderr.strip()} (swap error: {error})"
                ) from error
            branch = f"privileged swap after {error.strerror}"
        else:
            os.rename(target, backup)
            try:
                os.rename(staged, target)
            except OSError:
                os.rename(backup, target)
                raise
            subprocess.run(["rm", "-rf", str(backup)], check=False)
            branch = f"rename fallback after {error.strerror}"
    subprocess.run(["rm", "-rf", str(backup_dir)], check=False)
    os.utime(target)
    print(f"updater replace: {branch}; {target} is owned by uid {target.stat().st_uid}", flush=True)
    return branch


def read_bundle_info(bundle: pathlib.Path) -> dict[str, object]:
    with (bundle / "Contents/Info.plist").open("rb") as handle:
        return plistlib.load(handle)


def executable_path(bundle: pathlib.Path, executable: str) -> str:
    return str(bundle / "Contents/MacOS" / executable)


def get_process_path(pid: int) -> str:
    libproc = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
    buffer = ctypes.create_string_buffer(4096)
    if libproc.proc_pidpath(pid, buffer, ctypes.sizeof(buffer)) <= 0:
        return ""
    return os.fsdecode(buffer.value)


def list_app_processes() -> list[AppProcess]:
    processes = []
    for name in (EXECUTABLE, LEGACY_EXECUTABLE):
        result = run_command("pgrep", "-x", name)
        for line in result.stdout.split():
            pid = int(line)
            path = get_process_path(pid)
            if path:
                processes.append(AppProcess(pid, path))
    return processes


def is_settled(processes: list[AppProcess], expected_path: str) -> bool:
    return [process.path for process in processes] == [expected_path]


def wait_for_settled_app(expected_path: str, timeout: float = LAUNCH_TIMEOUT) -> list[AppProcess]:
    deadline = time.monotonic() + timeout
    settled_since: float | None = None
    processes: list[AppProcess] = []
    while time.monotonic() < deadline:
        processes = list_app_processes()
        if is_settled(processes, expected_path) and LEGACY_BUNDLE.is_symlink():
            settled_since = settled_since or time.monotonic()
            if time.monotonic() - settled_since >= STABLE_SECONDS:
                return processes
        else:
            settled_since = None
        time.sleep(1)
    return processes


def signal_app_processes(sent: signal.Signals, timeout: float) -> list[AppProcess]:
    for process in list_app_processes():
        try:
            os.kill(process.pid, sent)
        except ProcessLookupError:
            continue
    deadline = time.monotonic() + timeout
    remaining = list_app_processes()
    while remaining and time.monotonic() < deadline:
        time.sleep(1)
        remaining = list_app_processes()
    return remaining


def stop_app_processes(timeout: float = STOP_TIMEOUT) -> None:
    if not signal_app_processes(signal.SIGTERM, timeout):
        return
    remaining = signal_app_processes(signal.SIGKILL, KILL_TIMEOUT)
    if remaining:
        raise MacosCheckError(f"app processes still run after SIGTERM and SIGKILL: {remaining}")


def describe_state() -> str:
    lines = ["/Applications entries:"]
    for entry in sorted(APPLICATIONS.iterdir()):
        if entry.name in (BUNDLE_NAME, LEGACY_BUNDLE_NAME):
            status = entry.lstat()
            kind = f"link to {os.readlink(entry)}" if entry.is_symlink() else "folder"
            lines.append(f"  {entry.name}: {kind}, uid {status.st_uid}, flags {status.st_flags:#x}")
    lines.append(f"app processes: {list_app_processes()}")
    for folder in LOG_FOLDERS:
        logs = sorted(folder.glob("*"), key=lambda path: path.stat().st_mtime) if folder.is_dir() else []
        if logs:
            tail = logs[-1].read_text(errors="replace").splitlines()[-40:]
            lines.append(f"tail of {logs[-1]}:")
            lines.extend(f"  {line}" for line in tail)
    return "\n".join(lines)


def install_previous(previous: pathlib.Path, owner: str) -> None:
    for path in (BUNDLE, LEGACY_BUNDLE):
        if path.exists() or path.is_symlink():
            raise MacosCheckError(f"{path} exists before the previous release is installed")
    if owner == "admin":
        require_output(run_command("sudo", "-n", "installer", "-pkg", str(previous), "-target", "/", timeout=INSTALL_TIMEOUT))
    else:
        require_output(run_command("tar", "-xzf", str(previous), "-C", str(APPLICATIONS), timeout=INSTALL_TIMEOUT))
    if not LEGACY_BUNDLE.is_dir():
        raise MacosCheckError(f"{previous.name} left no {LEGACY_BUNDLE}")
    expected_uid = 0 if owner == "admin" else os.getuid()
    if LEGACY_BUNDLE.stat().st_uid != expected_uid:
        raise MacosCheckError(f"{LEGACY_BUNDLE} is owned by uid {LEGACY_BUNDLE.stat().st_uid}, not {expected_uid}")
    print(f"previous release installed: {LEGACY_BUNDLE} owned by uid {expected_uid}", flush=True)


def read_response(lines: list[str], request_id: int) -> dict[str, object] | None:
    for line in lines:
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(message, dict) and message.get("id") == request_id:
            return message
    return None


def exchange_mcp(command: str) -> tuple[dict[str, object], dict[str, object]]:
    process = subprocess.Popen(
        [command, "mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        start_new_session=True,
    )
    assert process.stdin is not None and process.stdout is not None
    received: list[str] = []

    def request(message: dict[str, object]) -> None:
        process.stdin.write(json.dumps(message) + "\n")
        process.stdin.flush()

    def await_response(request_id: int) -> dict[str, object]:
        deadline = time.monotonic() + MCP_TIMEOUT
        while time.monotonic() < deadline:
            ready, _, _ = select.select([process.stdout], [], [], 1)
            if ready:
                line = process.stdout.readline()
                if not line:
                    break
                received.append(line)
                response = read_response([line], request_id)
                if response is not None:
                    return response
        stop_process_tree(process, KILL_TIMEOUT)
        try:
            _, stderr = process.communicate(timeout=KILL_TIMEOUT)
        except subprocess.TimeoutExpired:
            stderr = f"(stderr was still open {KILL_TIMEOUT:.0f}s after the process group was killed)"
        raise MacosCheckError(f"{command} mcp sent no answer to request {request_id}; stdout {received}; stderr {stderr}")

    try:
        request(
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "upgrade-check", "version": "1"},
                },
            }
        )
        initialized = await_response(1)
        request({"jsonrpc": "2.0", "method": "notifications/initialized"})
        request({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
        tools = await_response(2)
        return initialized, tools
    finally:
        if process.poll() is None:
            process.stdin.close()
            try:
                process.wait(timeout=KILL_TIMEOUT)
            except subprocess.TimeoutExpired:
                pass
        stop_process_tree(process, KILL_TIMEOUT)


class ParseTests(unittest.TestCase):
    def test_a_quoted_path_survives_an_apostrophe(self) -> None:
        self.assertEqual(sh_quote("/tmp/it's here"), "'/tmp/it'\\''s here'")
        result = subprocess.run(["/bin/sh", "-c", f"printf %s {sh_quote(chr(39) + 'a b' + chr(39))}"], capture_output=True, text=True)
        self.assertEqual(result.stdout, "'a b'")

    def test_the_privileged_swap_falls_back_to_moves_and_restores_on_failure(self) -> None:
        command = privileged_swap_command(
            pathlib.Path("/tmp/tauri_updated_app1"), pathlib.Path("/Applications/Writ.app"), pathlib.Path("/tmp/c/current_app")
        )
        self.assertTrue(command.startswith('case "$(/usr/bin/osascript -l JavaScript -e '))
        self.assertIn("swapped) rm -rf '/tmp/tauri_updated_app1';;", command)
        self.assertIn(
            "failed) mv -f '/Applications/Writ.app' '/tmp/c/current_app' && { mv -f '/tmp/tauri_updated_app1' "
            "'/Applications/Writ.app' || { mv -f '/tmp/c/current_app' '/Applications/Writ.app'; exit 1; }; } "
            "&& rm -rf '/tmp/c/current_app';;",
            command,
        )
        self.assertTrue(command.endswith("*) exit 1;; esac"))
        self.assertEqual(subprocess.run(["/bin/sh", "-n", "-c", command]).returncode, 0)

    def test_the_privileged_swap_runs_both_branches_in_a_shell(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            base = pathlib.Path(folder)
            for answer, expected in (("swapped", "old"), ("failed", "new")):
                staged, target, backup = base / f"{answer}-staged", base / f"{answer}-target", base / f"{answer}-backup"
                staged.mkdir()
                target.mkdir()
                (staged / "marker").write_text("new")
                (target / "marker").write_text("old")
                command = privileged_swap_command(staged, target, backup).replace(
                    "/usr/bin/osascript", str(base / "osascript")
                )
                (base / "osascript").write_text(f"#!/bin/sh\necho {answer}\n")
                (base / "osascript").chmod(0o755)
                self.assertEqual(subprocess.run(["/bin/sh", "-c", command]).returncode, 0)
                self.assertFalse(staged.exists())
                self.assertFalse(backup.exists())
                self.assertEqual((target / "marker").read_text(), expected)

    def test_the_temp_root_is_the_first_folder_on_the_bundles_device(self) -> None:
        devices = {"/tmp": 1, "/Applications": 2}

        def fake_stat(path: pathlib.Path) -> os.stat_result:
            return os.stat_result((0, 0, devices[str(path)], 0, 0, 0, 0, 0, 0, 0))

        with tempfile.TemporaryDirectory() as folder:
            target = pathlib.Path(folder)
            devices["/Applications"] = target.lstat().st_dev
            devices["/tmp"] = target.lstat().st_dev + 1
            chosen = choose_temp_root(target, [pathlib.Path("/tmp"), pathlib.Path("/Applications")], fake_stat)
            self.assertEqual(chosen, pathlib.Path("/Applications"))

    def test_the_first_path_component_is_dropped(self) -> None:
        self.assertEqual(strip_first_component("Jotra.app/Contents/Info.plist"), "Contents/Info.plist")
        self.assertIsNone(strip_first_component("Jotra.app"))
        self.assertIsNone(strip_first_component("Jotra.app/"))

    def test_an_update_archive_extracts_without_its_top_folder(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            base = pathlib.Path(folder)
            app = base / "src/Jotra.app/Contents/MacOS"
            app.mkdir(parents=True)
            (app / EXECUTABLE).write_text("#!/bin/sh\n")
            (app / EXECUTABLE).chmod(0o755)
            os.symlink(EXECUTABLE, app / "alias")
            archive = base / "update.tar.gz"
            with tarfile.open(archive, "w:gz") as tar:
                tar.add(base / "src/Jotra.app", arcname="Jotra.app")
            staged = pathlib.Path(tempfile.mkdtemp(prefix=UPDATED_APP_PREFIX, dir=base))
            extract_update(archive, staged)
            self.assertTrue((staged / "Contents/MacOS" / EXECUTABLE).is_file())
            self.assertEqual(os.readlink(staged / "Contents/MacOS/alias"), EXECUTABLE)
            self.assertEqual(stat.S_IMODE(staged.stat().st_mode), 0o755)

    def test_bundle_info_reads_the_plist(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            bundle = pathlib.Path(folder, BUNDLE_NAME)
            (bundle / "Contents").mkdir(parents=True)
            with (bundle / "Contents/Info.plist").open("wb") as handle:
                plistlib.dump({"CFBundleIdentifier": IDENTIFIER, "CFBundleExecutable": EXECUTABLE}, handle)
            self.assertEqual(read_bundle_info(bundle)["CFBundleIdentifier"], IDENTIFIER)

    def test_one_process_from_the_new_bundle_is_settled(self) -> None:
        expected = executable_path(BUNDLE, EXECUTABLE)
        self.assertTrue(is_settled([AppProcess(1, expected)], expected))
        self.assertFalse(is_settled([], expected))
        self.assertFalse(is_settled([AppProcess(1, expected), AppProcess(2, expected)], expected))
        self.assertFalse(is_settled([AppProcess(1, executable_path(LEGACY_BUNDLE, EXECUTABLE))], expected))

    def test_a_response_is_found_by_id_among_other_lines(self) -> None:
        lines = ['{"jsonrpc":"2.0","method":"notifications/message"}', "not json", '{"jsonrpc":"2.0","id":2,"result":{}}']
        self.assertEqual(read_response(lines, 2), {"jsonrpc": "2.0", "id": 2, "result": {}})
        self.assertIsNone(read_response(lines, 1))


class UpgradeTests(unittest.TestCase):
    version: str
    owner: str
    branch: str

    @classmethod
    def setUpClass(cls) -> None:
        previous = get_artifact_path(FROM_VARIABLE)
        current = get_artifact_path(TO_VARIABLE)
        owner = os.environ.get(OWNER_VARIABLE, "")
        version = os.environ.get(VERSION_VARIABLE, "")
        if previous is None or current is None or not owner or not version:
            raise unittest.SkipTest(f"{FROM_VARIABLE}, {TO_VARIABLE}, {OWNER_VARIABLE} and {VERSION_VARIABLE} are not all set")
        if sys.platform != "darwin":
            raise unittest.SkipTest("the bundle replace runs on macOS only")
        if owner not in OWNERS:
            raise MacosCheckError(f"{OWNER_VARIABLE} is {owner}, not one of {OWNERS}")
        cls.version = version
        cls.owner = owner
        try:
            cls.start_upgraded_app(previous, current, owner)
        except BaseException:
            stop_app_processes()
            raise

    @classmethod
    def start_upgraded_app(cls, previous: pathlib.Path, current: pathlib.Path, owner: str) -> None:
        install_previous(previous, owner)
        require_output(run_command("/usr/bin/open", str(LEGACY_BUNDLE)))
        legacy_path = executable_path(LEGACY_BUNDLE, LEGACY_EXECUTABLE)
        deadline = time.monotonic() + LAUNCH_TIMEOUT
        while time.monotonic() < deadline and legacy_path not in [p.path for p in list_app_processes()]:
            time.sleep(1)
        if legacy_path not in [p.path for p in list_app_processes()]:
            raise MacosCheckError(f"the previous release did not start from {legacy_path} within {LAUNCH_TIMEOUT:.0f}s\n{describe_state()}")
        print(f"previous release running: {list_app_processes()}", flush=True)

        cls.branch = replace_bundle(current, LEGACY_BUNDLE)
        stop_app_processes()
        require_output(run_command("/usr/bin/open", "-n", str(LEGACY_BUNDLE)))
        settled = wait_for_settled_app(executable_path(BUNDLE, EXECUTABLE))
        print(f"after the relaunch: {settled}", flush=True)

    @classmethod
    def tearDownClass(cls) -> None:
        if sys.platform == "darwin" and os.environ.get(TO_VARIABLE):
            stop_app_processes()

    def test_the_bundle_carries_the_new_name(self) -> None:
        self.assertTrue(BUNDLE.is_dir() and not BUNDLE.is_symlink(), f"{BUNDLE} is not a folder\n{describe_state()}")
        info = read_bundle_info(BUNDLE)
        expected = {"CFBundleIdentifier": IDENTIFIER, "CFBundleShortVersionString": self.version, "CFBundleExecutable": EXECUTABLE}
        found = {key: info.get(key) for key in expected}
        self.assertEqual(found, expected, f"{BUNDLE}/Contents/Info.plist")

    def test_a_hidden_link_keeps_the_old_name(self) -> None:
        self.assertTrue(LEGACY_BUNDLE.is_symlink(), f"{LEGACY_BUNDLE} is not a link\n{describe_state()}")
        self.assertEqual(os.readlink(LEGACY_BUNDLE), BUNDLE_NAME, f"{LEGACY_BUNDLE} points at {os.readlink(LEGACY_BUNDLE)}")
        flags = LEGACY_BUNDLE.lstat().st_flags
        self.assertTrue(flags & stat.UF_HIDDEN, f"{LEGACY_BUNDLE} flags are {flags:#x}, without UF_HIDDEN")

    def test_exactly_one_app_process_runs_from_the_new_bundle(self) -> None:
        expected = executable_path(BUNDLE, EXECUTABLE)
        processes = list_app_processes()
        self.assertTrue(is_settled(processes, expected), f"app processes after the upgrade: {processes}\n{describe_state()}")

    def test_the_bundle_identifier_opens_the_new_bundle(self) -> None:
        expected = executable_path(BUNDLE, EXECUTABLE)
        before = list_app_processes()
        require_output(run_command("/usr/bin/open", "-b", IDENTIFIER))
        time.sleep(STABLE_SECONDS)
        running = list_app_processes()
        self.assertEqual(
            [p.pid for p in running], [p.pid for p in before], f"open -b {IDENTIFIER} while running changed {before} to {running}"
        )
        stop_app_processes()
        require_output(run_command("/usr/bin/open", "-b", IDENTIFIER))
        settled = wait_for_settled_app(expected)
        self.assertTrue(is_settled(settled, expected), f"open -b {IDENTIFIER} after quitting started {settled}\n{describe_state()}")

    def test_the_old_command_serves_mcp_through_the_link(self) -> None:
        command = str(LEGACY_BUNDLE / "Contents/MacOS" / LEGACY_COMMAND)
        initialized, tools = exchange_mcp(command)
        server = (initialized.get("result") or {}).get("serverInfo") or {}
        self.assertEqual(
            (server.get("name"), server.get("version")),
            (LEGACY_SERVER_NAME, self.version),
            f"{command} mcp initialize answered {initialized}",
        )
        listed = (tools.get("result") or {}).get("tools") or []
        self.assertTrue(listed, f"{command} mcp tools/list answered {tools}")


def main() -> int:
    suite = unittest.defaultTestLoader.loadTestsFromModule(sys.modules[__name__])
    result = unittest.TextTestRunner(verbosity=2, resultclass=ImmediateTextResult).run(suite)
    for test, reason in result.skipped:
        print(f"skipped {test}: {reason}", file=sys.stderr)
    return 0 if result.wasSuccessful() and not result.skipped else 1


if __name__ == "__main__":
    sys.exit(main())
