"""Check the desktop integration a deb update carries over from the previous release.

The release workflow runs this on a disposable Ubuntu runner, where it installs
packages with sudo and writes to the runner user's home folder:
    DEB_UPGRADE_FROM=<previous .deb> DEB_UPGRADE_TO=<new .deb> \\
        python3 .github/scripts/test_deb_upgrade_desktop.py

It sets the previous release as the default Markdown handler and leaves a
turned-off login item under the old name, updates with apt, then reads the
handler under GNOME and KDE and starts the new app once in a virtual display
so it moves the login item. Without those variables the tests are skipped. Run
as a script, a skipped test fails the run.
"""

from __future__ import annotations

import os
import pathlib
import shlex
import shutil
import subprocess
import sys
import time
import unittest

from check_process import ImmediateTextResult, StartedProcess, run_bounded, start_process
from test_deb_upgrade_legacy import get_deb_path, get_package_status, parse_desktop_entry, read_control_fields

LEGACY_PACKAGE = "writ"
PACKAGE = "jotra"
MIME_TYPE = "text/markdown"
LEGACY_DESKTOP_FILE = "Writ.desktop"
APPLICATIONS_DIR = pathlib.Path("/usr/share/applications")
LAUNCHER = "/usr/bin/jotra-tauri"
LEGACY_LAUNCHER = "/usr/bin/writ-tauri"
AUTOSTART_DIR = pathlib.Path.home() / ".config/autostart"
LEGACY_AUTOSTART = AUTOSTART_DIR / "Writ.desktop"
AUTOSTART = AUTOSTART_DIR / "Jotra.desktop"
PLUGIN_AUTOSTART_ENTRY = (
    "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Writ\nComment=Writ startup script\n"
    f"Exec={LEGACY_LAUNCHER} --login\nStartupNotify=false\nTerminal=false\n"
)
SEEDED_KEYS = {"Hidden": "false", "X-GNOME-Autostart-enabled": "false", "X-Upgrade-Check": "kept"}
DESKTOPS = {
    "GNOME": {"XDG_CURRENT_DESKTOP": "GNOME", "DESKTOP_SESSION": "gnome"},
    "KDE": {"XDG_CURRENT_DESKTOP": "KDE", "DESKTOP_SESSION": "plasma", "KDE_SESSION_VERSION": "5"},
}
DESKTOP_VARIABLES = ("XDG_CURRENT_DESKTOP", "DESKTOP_SESSION", "KDE_SESSION_VERSION", "KDE_FULL_SESSION", "GNOME_DESKTOP_SESSION_ID")
TOOLS = ("xdg-utils", "libglib2.0-bin", "kde-cli-tools", "libkf5service-bin", "xvfb", "xauth", "dbus", "dbus-x11")
FROM_VARIABLE = "DEB_UPGRADE_FROM"
TO_VARIABLE = "DEB_UPGRADE_TO"
LAUNCH_TIMEOUT = 90.0
COMMAND_TIMEOUT = 120.0
INSTALL_TIMEOUT = 300.0


class DesktopCheckError(RuntimeError):
    pass


def run_command(*args: str, env: dict[str, str] | None = None, timeout: float = COMMAND_TIMEOUT) -> subprocess.CompletedProcess[str]:
    return run_bounded(args, timeout=timeout, env=env)


def require_output(result: subprocess.CompletedProcess[str]) -> str:
    if result.returncode != 0:
        command = " ".join(str(arg) for arg in result.args)
        raise DesktopCheckError(f"{command} exited {result.returncode}: {result.stderr.strip()}")
    return result.stdout


def seeded_autostart_entry() -> str:
    return PLUGIN_AUTOSTART_ENTRY + "".join(f"{key}={value}\n" for key, value in SEEDED_KEYS.items())


def expected_autostart_keys(seeded: str) -> dict[str, str]:
    keys = parse_desktop_entry(seeded)
    keys["Exec"] = f"{LAUNCHER} --login"
    keys["Name"] = "Jotra"
    keys["Comment"] = "Jotra startup script"
    return keys


def desktop_environment(desktop: str, base: dict[str, str] | None = None) -> dict[str, str]:
    env = {key: value for key, value in (base or os.environ).items() if key not in DESKTOP_VARIABLES}
    env.update(DESKTOPS[desktop])
    env["QT_QPA_PLATFORM"] = "offscreen"
    return env


def exec_program(exec_value: str) -> str:
    words = shlex.split(exec_value)
    return words[0] if words else ""


def query_default(env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    return run_command("xdg-mime", "query", "default", MIME_TYPE, env=env)


def describe_mime_state(desktop: str) -> str:
    env = {**desktop_environment(desktop), "XDG_UTILS_DEBUG_LEVEL": "3"}
    debug = query_default(env)
    lines = [f"xdg-mime under {desktop} (debug): stdout {debug.stdout.strip()!r} stderr {debug.stderr.strip()!r}"]
    for path in (pathlib.Path.home() / ".config/mimeapps.list", APPLICATIONS_DIR / "mimeinfo.cache"):
        if path.is_file():
            matching = [line for line in path.read_text().splitlines() if MIME_TYPE in line or line.startswith("[")]
            lines.append(f"{path}: {matching}")
    if desktop == "KDE" and shutil.which("ktraderclient5"):
        trader = run_command("ktraderclient5", "--mimetype", MIME_TYPE, "--servicetype", "Application", env=env)
        paths = [line.strip() for line in trader.stdout.splitlines() if "DesktopEntryPath" in line]
        lines.append(f"ktraderclient5: {paths} {trader.stderr.strip()[-400:]}")
    return "\n".join(lines)


def start_app() -> StartedProcess:
    env = {**desktop_environment("GNOME"), "WEBKIT_DISABLE_COMPOSITING_MODE": "1", "WEBKIT_DISABLE_DMABUF_RENDERER": "1"}
    env.pop("QT_QPA_PLATFORM")
    return start_process(["dbus-run-session", "--", "xvfb-run", "-a", LAUNCHER], env=env, merge_output=True)


class ParseTests(unittest.TestCase):
    def test_the_seeded_login_item_reads_as_the_plugin_wrote_it_plus_the_kept_keys(self) -> None:
        keys = parse_desktop_entry(seeded_autostart_entry())
        self.assertEqual(keys["Exec"], f"{LEGACY_LAUNCHER} --login")
        self.assertEqual(keys["Name"], "Writ")
        self.assertEqual(keys["X-GNOME-Autostart-enabled"], "false")

    def test_the_moved_login_item_keeps_every_other_key(self) -> None:
        expected = expected_autostart_keys(seeded_autostart_entry())
        self.assertEqual(expected["Exec"], "/usr/bin/jotra-tauri --login")
        self.assertEqual(expected["Comment"], "Jotra startup script")
        for key, value in SEEDED_KEYS.items():
            self.assertEqual(expected[key], value)
        self.assertEqual(expected["StartupNotify"], "false")

    def test_a_desktop_environment_replaces_the_session_variables(self) -> None:
        base = {"PATH": "/bin", "XDG_CURRENT_DESKTOP": "ubuntu:GNOME", "KDE_FULL_SESSION": "true"}
        kde = desktop_environment("KDE", base)
        self.assertEqual(kde["XDG_CURRENT_DESKTOP"], "KDE")
        self.assertEqual(kde["KDE_SESSION_VERSION"], "5")
        self.assertNotIn("KDE_FULL_SESSION", kde)
        gnome = desktop_environment("GNOME", base)
        self.assertEqual(gnome["XDG_CURRENT_DESKTOP"], "GNOME")
        self.assertNotIn("KDE_SESSION_VERSION", gnome)
        self.assertEqual(gnome["PATH"], "/bin")

    def test_the_program_is_the_first_word_of_exec(self) -> None:
        self.assertEqual(exec_program("jotra-tauri %F"), "jotra-tauri")
        self.assertEqual(exec_program('"/opt/My App/run" --login'), "/opt/My App/run")
        self.assertEqual(exec_program(""), "")


class UpgradeTests(unittest.TestCase):
    seeded: str
    handlers: dict[str, tuple[str, str]]

    @classmethod
    def setUpClass(cls) -> None:
        previous = get_deb_path(FROM_VARIABLE)
        current = get_deb_path(TO_VARIABLE)
        if previous is None or current is None:
            raise unittest.SkipTest(f"{FROM_VARIABLE} and {TO_VARIABLE} are not both set")
        if shutil.which("apt-get") is None or shutil.which("sudo") is None:
            raise unittest.SkipTest("apt-get and sudo are required")
        previous_package = read_control_fields(previous).get("Package")
        if previous_package != LEGACY_PACKAGE:
            raise DesktopCheckError(f"{previous.name} is package {previous_package}, not {LEGACY_PACKAGE}")
        require_output(run_command("sudo", "apt-get", "update", timeout=INSTALL_TIMEOUT))
        require_output(run_command("sudo", "apt-get", "install", "--yes", "--no-install-recommends", *TOOLS, timeout=INSTALL_TIMEOUT))
        require_output(run_command("sudo", "apt-get", "install", "--yes", str(previous), timeout=INSTALL_TIMEOUT))
        if get_package_status(LEGACY_PACKAGE) != "installed":
            raise DesktopCheckError(f"{previous.name} left {LEGACY_PACKAGE} {get_package_status(LEGACY_PACKAGE)}")

        require_output(run_command("xdg-mime", "default", LEGACY_DESKTOP_FILE, MIME_TYPE, env=desktop_environment("GNOME")))
        before = query_default(desktop_environment("GNOME")).stdout.strip()
        if before != LEGACY_DESKTOP_FILE:
            raise DesktopCheckError(f"xdg-mime reads {before!r} as the {MIME_TYPE} handler before the update\n{describe_mime_state('GNOME')}")
        AUTOSTART_DIR.mkdir(parents=True, exist_ok=True)
        if AUTOSTART.exists():
            raise DesktopCheckError(f"{AUTOSTART} exists before the update")
        cls.seeded = seeded_autostart_entry()
        LEGACY_AUTOSTART.write_text(cls.seeded)
        print(f"previous release installed; {MIME_TYPE} opens with {before}; login item at {LEGACY_AUTOSTART}", flush=True)

        require_output(run_command("sudo", "apt-get", "install", "--yes", str(current), timeout=INSTALL_TIMEOUT))
        if get_package_status(PACKAGE) != "installed":
            raise DesktopCheckError(f"{current.name} left {PACKAGE} {get_package_status(PACKAGE)}")
        if shutil.which("kbuildsycoca5"):
            rebuilt = run_command("kbuildsycoca5", "--noincremental", env=desktop_environment("KDE"))
            print(f"kbuildsycoca5 exited {rebuilt.returncode}", flush=True)
        cls.handlers = {}
        for desktop in DESKTOPS:
            answer = query_default(desktop_environment(desktop)).stdout.strip()
            cls.handlers[desktop] = (answer, "" if answer == LEGACY_DESKTOP_FILE else describe_mime_state(desktop))

    def test_the_markdown_handler_survives_the_update(self) -> None:
        for desktop, (answer, state) in self.handlers.items():
            with self.subTest(desktop=desktop):
                self.assertEqual(
                    answer, LEGACY_DESKTOP_FILE, f"xdg-mime query default {MIME_TYPE} under {desktop} after the update\n{state}"
                )

    def test_the_kept_handler_opens_the_new_app(self) -> None:
        path = APPLICATIONS_DIR / LEGACY_DESKTOP_FILE
        self.assertTrue(path.is_file(), f"{path} is gone after the update")
        keys = parse_desktop_entry(path.read_text())
        self.assertEqual(keys.get("NoDisplay"), "true", f"{path}: {keys}")
        self.assertIn(MIME_TYPE, keys.get("MimeType", "").split(";"), f"{path}: {keys}")
        program = exec_program(keys.get("Exec", ""))
        resolved = shutil.which(program) if program else None
        self.assertEqual(
            os.path.realpath(resolved) if resolved else None, LAUNCHER, f"{path} Exec {keys.get('Exec')!r} resolves to {resolved}"
        )

    def test_the_login_item_moves_to_the_new_name(self) -> None:
        started = start_app()
        try:
            deadline = time.monotonic() + LAUNCH_TIMEOUT
            while time.monotonic() < deadline and started.process.poll() is None:
                if AUTOSTART.is_file() and not LEGACY_AUTOSTART.exists():
                    break
                time.sleep(1)
            exited = started.process.poll()
        finally:
            output, _ = started.stop()
        context = f"app exit {exited}; output tail:\n{output[-3000:]}"
        self.assertTrue(AUTOSTART.is_file(), f"no {AUTOSTART} {LAUNCH_TIMEOUT:.0f}s after launch; {context}")
        self.assertFalse(LEGACY_AUTOSTART.exists(), f"{LEGACY_AUTOSTART} is still there; {context}")
        found = parse_desktop_entry(AUTOSTART.read_text())
        self.assertEqual(found, expected_autostart_keys(self.seeded), f"{AUTOSTART} after the move")


def main() -> int:
    suite = unittest.defaultTestLoader.loadTestsFromModule(sys.modules[__name__])
    result = unittest.TextTestRunner(verbosity=2, resultclass=ImmediateTextResult).run(suite)
    for test, reason in result.skipped:
        print(f"skipped {test}: {reason}", file=sys.stderr)
    return 0 if result.wasSuccessful() and not result.skipped else 1


if __name__ == "__main__":
    sys.exit(main())
