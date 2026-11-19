"""Run the previous release's install.sh, then this one, against the real release assets.

The release workflow runs this on a disposable Ubuntu runner:
    INSTALL_SH_PREVIOUS_SCRIPT=<previous install.sh> INSTALL_SH_PREVIOUS_ASSETS=<folder> \\
    INSTALL_SH_PREVIOUS_VERSION=<previous version> \\
    INSTALL_SH_ASSETS=<folder> INSTALL_SH_VERSION=<new version> \\
        python3 .github/scripts/test_install_sh_upgrade.py

Each assets folder holds that release's AppImage, its .sig, SHA256SUMS.txt and
SHA256SUMS.txt.minisig. A stub curl answers both scripts from those folders, so
each script checks its own release's signatures with the key it carries.
INSTALL_SH_SCRIPT runs another script in place of the repository's install.sh;
INSTALL_SH_PUBKEY replaces the key in the copy of it that runs. Without the
five variables the tests are skipped. Run as a script, a skipped test fails the
run.
"""

from __future__ import annotations

import dataclasses
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
DEFAULT_SCRIPT = REPO_ROOT / "install.sh"
KEY_LINE = re.compile(r'^MINISIGN_PUBKEY="[A-Za-z0-9+/=]+"$', re.MULTILINE)
PREVIOUS_SCRIPT_VARIABLE = "INSTALL_SH_PREVIOUS_SCRIPT"
PREVIOUS_ASSETS_VARIABLE = "INSTALL_SH_PREVIOUS_ASSETS"
PREVIOUS_VERSION_VARIABLE = "INSTALL_SH_PREVIOUS_VERSION"
ASSETS_VARIABLE = "INSTALL_SH_ASSETS"
VERSION_VARIABLE = "INSTALL_SH_VERSION"
SCRIPT_VARIABLE = "INSTALL_SH_SCRIPT"
PUBKEY_VARIABLE = "INSTALL_SH_PUBKEY"
RELEASE_FILES = ("{asset}", "{asset}.sig", "SHA256SUMS.txt", "SHA256SUMS.txt.minisig")
LEGACY_LAYOUT = ".local/share/writ"
LAYOUT = ".local/share/jotra"
BIN_DIR = ".local/bin"

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
  printf 'HTTP/2 302\\r\\nlocation: https://github.com/ibrahemid/writ/releases/tag/%s\\r\\n\\r\\n' "$FAKE_TAG"
  exit 0
fi
name="${url##*/}"
rest="${url%/*}"
tag="${rest##*/}"
[ -f "$FIXTURES/$tag/$name" ] || { echo "no fixture $tag/$name" >> "$CURL_LOG"; exit 22; }
cp "$FIXTURES/$tag/$name" "$out"
"""


class InstallCheckError(RuntimeError):
    pass


@dataclasses.dataclass(frozen=True)
class Wrapper:
    gui: str
    cli: str
    command_name: str | None


@dataclasses.dataclass(frozen=True)
class Release:
    tag: str
    version: str
    asset: str
    folder: pathlib.Path


def get_folder(variable: str) -> pathlib.Path | None:
    value = os.environ.get(variable, "")
    if not value:
        return None
    path = pathlib.Path(value).resolve()
    if not path.is_dir():
        raise InstallCheckError(f"{variable} names {value}, which is not a folder")
    return path


def get_file(variable: str, default: pathlib.Path | None = None) -> pathlib.Path | None:
    value = os.environ.get(variable, "")
    if not value:
        return default
    path = pathlib.Path(value).resolve()
    if not path.is_file():
        raise InstallCheckError(f"{variable} names {value}, which is not a file")
    return path


def asset_name(product: str, version: str) -> str:
    return f"{product}_{version}_amd64.AppImage"


def with_pubkey(script: str, pubkey: str) -> str:
    replaced, count = KEY_LINE.subn(f'MINISIGN_PUBKEY="{pubkey}"', script, count=1)
    if count != 1:
        raise InstallCheckError("the install script has no MINISIGN_PUBKEY line to replace")
    return replaced


def parse_wrapper(text: str) -> Wrapper:
    gui = re.search(r'JOTRA_GUI_BIN="([^"$]*)"', text) or re.search(r'WRIT_GUI_BIN="([^"$]*)"', text)
    cli = re.search(r'^exec "([^"]*)" "\$@"$', text, re.MULTILINE)
    name = re.search(r'^JOTRA_COMMAND_NAME="([^"]*)"; export JOTRA_COMMAND_NAME$', text, re.MULTILINE)
    if gui is None or cli is None:
        raise InstallCheckError(f"the command wrapper names no app or no command:\n{text}")
    return Wrapper(gui.group(1), cli.group(1), name.group(1) if name else None)


def stage_release(fixtures: pathlib.Path, release: Release) -> None:
    folder = fixtures / release.tag
    folder.mkdir(parents=True)
    for pattern in RELEASE_FILES:
        name = pattern.format(asset=release.asset)
        source = release.folder / name
        if not source.is_file():
            raise InstallCheckError(f"{release.folder} has no {name}")
        shutil.copyfile(source, folder / name)


def make_host(root: pathlib.Path) -> dict[str, str]:
    tools = root / "tools"
    tools.mkdir()
    curl = tools / "curl"
    curl.write_text(CURL)
    curl.chmod(0o755)
    home = root / "home"
    home.mkdir()
    return {
        "PATH": f"{tools}{os.pathsep}{os.environ.get('PATH', '/usr/bin:/bin')}",
        "HOME": str(home),
        "FIXTURES": str(root / "fixtures"),
        "CURL_LOG": str(root / "curl.log"),
        "LANG": "C.UTF-8",
    }


def run_script(script: pathlib.Path, env: dict[str, str], tag: str) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(["sh", str(script)], capture_output=True, text=True, env={**env, "FAKE_TAG": tag}, check=False)
    print(f"sh {script.name} for {tag} exited {result.returncode}\n{result.stdout}{result.stderr}", flush=True)
    return result


def require_install(result: subprocess.CompletedProcess[str], version: str, env: dict[str, str], product: str) -> None:
    expected = f"{product} install: installed {version}"
    if "signature is not checked" in result.stderr:
        raise InstallCheckError(f"{product} install.sh found no signature checker and skipped the check:\n{result.stderr}")
    if result.returncode != 0 or expected not in result.stdout:
        log = pathlib.Path(env["CURL_LOG"])
        downloads = log.read_text() if log.is_file() else ""
        raise InstallCheckError(
            f"{product} install.sh did not print {expected!r} (exit {result.returncode})\n"
            f"stdout: {result.stdout}\nstderr: {result.stderr}\ncurl calls:\n{downloads}"
        )


def get_version_output(command: pathlib.Path, env: dict[str, str]) -> str:
    result = subprocess.run([str(command), "--version"], capture_output=True, text=True, env=env, check=False)
    return f"exit {result.returncode}: {result.stdout.strip()} {result.stderr.strip()}".strip()


class ParseTests(unittest.TestCase):
    def test_a_current_wrapper_names_its_app_command_and_old_name(self) -> None:
        text = (
            '#!/bin/sh\nJOTRA_GUI_BIN="${JOTRA_GUI_BIN:-${WRIT_GUI_BIN:-}}"\n'
            '[ -n "${JOTRA_GUI_BIN}" ] || JOTRA_GUI_BIN="/h/.local/share/writ/Writ.AppImage"\nexport JOTRA_GUI_BIN\n'
            'JOTRA_COMMAND_NAME="writ"; export JOTRA_COMMAND_NAME\nexec "/h/.local/share/writ/writ-cli" "$@"\n'
        )
        self.assertEqual(
            parse_wrapper(text), Wrapper("/h/.local/share/writ/Writ.AppImage", "/h/.local/share/writ/writ-cli", "writ")
        )

    def test_the_previous_wrapper_names_its_app_and_command(self) -> None:
        text = '#!/bin/sh\nWRIT_GUI_BIN="/h/a/Writ.AppImage"; export WRIT_GUI_BIN\nexec "/h/a/writ-cli" "$@"\n'
        self.assertEqual(parse_wrapper(text), Wrapper("/h/a/Writ.AppImage", "/h/a/writ-cli", None))

    def test_a_wrapper_without_a_command_is_refused(self) -> None:
        with self.assertRaises(InstallCheckError):
            parse_wrapper('#!/bin/sh\nJOTRA_GUI_BIN="/a"\n')

    def test_the_key_line_is_replaced_once(self) -> None:
        script = 'REPO="x"\nMINISIGN_PUBKEY="RWQold+/="\nother\n'
        self.assertEqual(with_pubkey(script, "RWQnew"), 'REPO="x"\nMINISIGN_PUBKEY="RWQnew"\nother\n')
        with self.assertRaises(InstallCheckError):
            with_pubkey("no key here\n", "RWQnew")

    def test_the_repository_script_carries_a_key_line(self) -> None:
        self.assertEqual(len(KEY_LINE.findall(DEFAULT_SCRIPT.read_text())), 1)

    def test_the_stub_curl_answers_each_tag_from_its_own_folder(self) -> None:
        with tempfile.TemporaryDirectory() as folder:
            root = pathlib.Path(folder)
            env = make_host(root)
            for tag, body in (("v1.0.1", "old"), ("v1.0.2", "new")):
                (root / "fixtures" / tag).mkdir(parents=True)
                (root / "fixtures" / tag / "SHA256SUMS.txt").write_text(body)
            head = subprocess.run(
                ["curl", "-fsSI", "https://github.com/ibrahemid/writ/releases/latest"],
                capture_output=True, text=True, env={**env, "FAKE_TAG": "v1.0.2"},
            )
            self.assertIn("location: https://github.com/ibrahemid/writ/releases/tag/v1.0.2", head.stdout.splitlines())
            out = root / "out"
            for tag, body in (("v1.0.1", "old"), ("v1.0.2", "new")):
                url = f"https://github.com/ibrahemid/writ/releases/download/{tag}/SHA256SUMS.txt"
                got = subprocess.run(["curl", "-fsSL", "-o", str(out), url], env={**env, "FAKE_TAG": tag})
                self.assertEqual(got.returncode, 0)
                self.assertEqual(out.read_text(), body)
            missing = subprocess.run(
                ["curl", "-fsSL", "-o", str(out), "https://github.com/ibrahemid/writ/releases/download/v1.0.2/none"],
                env={**env, "FAKE_TAG": "v1.0.2"},
            )
            self.assertEqual(missing.returncode, 22)


class UpgradeTests(unittest.TestCase):
    previous: Release
    current: Release
    previous_script: pathlib.Path
    script: pathlib.Path

    @classmethod
    def setUpClass(cls) -> None:
        previous_script = get_file(PREVIOUS_SCRIPT_VARIABLE)
        previous_folder = get_folder(PREVIOUS_ASSETS_VARIABLE)
        current_folder = get_folder(ASSETS_VARIABLE)
        previous_version = os.environ.get(PREVIOUS_VERSION_VARIABLE, "")
        version = os.environ.get(VERSION_VARIABLE, "")
        if not (previous_script and previous_folder and current_folder and previous_version and version):
            raise unittest.SkipTest(
                f"{PREVIOUS_SCRIPT_VARIABLE}, {PREVIOUS_ASSETS_VARIABLE}, {PREVIOUS_VERSION_VARIABLE}, "
                f"{ASSETS_VARIABLE} and {VERSION_VARIABLE} are not all set"
            )
        cls.previous = Release(f"v{previous_version}", previous_version, asset_name("Writ", previous_version), previous_folder)
        cls.current = Release(f"v{version}", version, asset_name("Jotra", version), current_folder)
        cls.previous_script = previous_script
        cls.work = pathlib.Path(tempfile.mkdtemp(prefix="install-sh-upgrade-"))
        script = get_file(SCRIPT_VARIABLE, DEFAULT_SCRIPT)
        assert script is not None
        pubkey = os.environ.get(PUBKEY_VARIABLE, "")
        cls.script = cls.work / "install.sh"
        cls.script.write_text(with_pubkey(script.read_text(), pubkey) if pubkey else script.read_text())

    @classmethod
    def tearDownClass(cls) -> None:
        shutil.rmtree(cls.work, ignore_errors=True)

    def make_host(self, name: str) -> tuple[dict[str, str], pathlib.Path]:
        root = self.work / name
        root.mkdir()
        env = make_host(root)
        for release in (self.previous, self.current):
            stage_release(pathlib.Path(env["FIXTURES"]), release)
        return env, pathlib.Path(env["HOME"])

    def expected_gui(self, env: dict[str, str], app: pathlib.Path, unpacked: pathlib.Path) -> pathlib.Path:
        return app if shutil.which("fusermount", path=env["PATH"]) else unpacked

    def assert_wrappers(self, home: pathlib.Path, env: dict[str, str], gui: pathlib.Path, cli: pathlib.Path) -> None:
        for command, command_name in (("jotra", None), ("writ", "writ")):
            with self.subTest(command=command):
                wrapper = home / BIN_DIR / command
                self.assertTrue(wrapper.is_file(), f"no {wrapper}")
                self.assertEqual(
                    parse_wrapper(wrapper.read_text()),
                    Wrapper(str(gui), str(cli), command_name),
                    f"{wrapper}:\n{wrapper.read_text()}",
                )
                output = get_version_output(wrapper, env)
                self.assertIn(self.current.version, output, f"{wrapper} --version: {output}")

    def test_a_previous_install_is_updated_in_place(self) -> None:
        env, home = self.make_host("legacy")
        require_install(run_script(self.previous_script, env, self.previous.tag), self.previous.version, env, "writ")
        layout = home / LEGACY_LAYOUT
        old_cli = layout / "writ-cli"
        self.assertIn(self.previous.version, get_version_output(old_cli, env), "the previous release's command")

        require_install(run_script(self.script, env, self.current.tag), self.current.version, env, "jotra")

        new_image = (self.current.folder / self.current.asset).read_bytes()
        self.assertEqual((layout / "Writ.AppImage").read_bytes(), new_image, f"{layout / 'Writ.AppImage'} is not the new AppImage")
        output = get_version_output(old_cli, env)
        self.assertIn(self.current.version, output, f"{old_cli} --version: {output}")
        gui = self.expected_gui(env, layout / "Writ.AppImage", layout / "writ-gui")
        self.assert_wrappers(home, env, gui, old_cli)
        if gui.name == "writ-gui":
            self.assertEqual(
                gui.read_text(), f'#!/bin/sh\nexec "{layout / "Writ.AppImage"}" --appimage-extract-and-run "$@"\n'
            )
        self.assertFalse((home / LAYOUT).exists(), f"the update also made {home / LAYOUT}")

    def test_a_fresh_install_uses_the_new_layout_and_both_commands(self) -> None:
        env, home = self.make_host("fresh")
        require_install(run_script(self.script, env, self.current.tag), self.current.version, env, "jotra")
        layout = home / LAYOUT
        new_image = (self.current.folder / self.current.asset).read_bytes()
        self.assertEqual((layout / "Jotra.AppImage").read_bytes(), new_image, f"{layout / 'Jotra.AppImage'} is not the new AppImage")
        cli = layout / "jotra-cli"
        output = get_version_output(cli, env)
        self.assertIn(self.current.version, output, f"{cli} --version: {output}")
        self.assert_wrappers(home, env, self.expected_gui(env, layout / "Jotra.AppImage", layout / "jotra-gui"), cli)
        self.assertFalse((home / LEGACY_LAYOUT).exists(), f"a fresh install made {home / LEGACY_LAYOUT}")


def main() -> int:
    suite = unittest.defaultTestLoader.loadTestsFromModule(sys.modules[__name__])
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    for test, reason in result.skipped:
        print(f"skipped {test}: {reason}", file=sys.stderr)
    return 0 if result.wasSuccessful() and not result.skipped else 1


if __name__ == "__main__":
    sys.exit(main())
