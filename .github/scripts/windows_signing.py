#!/usr/bin/env python3
"""Writes the Tauri config overlay that signs the Windows build through bundle.windows.signCommand.

The sign command is signtool with the Azure Artifact Signing dlib, which signs
with a certificate profile in an Artifact Signing account and authenticates
from AZURE_TENANT_ID, AZURE_CLIENT_ID and AZURE_CLIENT_SECRET in the environment.
The bundler runs it on the sidecar, on the main executable and on the MSI;
the sign subcommand runs the same command on a file the bundler does not sign.

Run from the repository root:
    python .github/scripts/windows_signing.py --client-package client.nupkg --signtool PATH \\
        --endpoint URL --account NAME --profile NAME --out-dir DIR
    python .github/scripts/windows_signing.py sign CONFIG FILE
"""
from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys
import zipfile
from collections.abc import Callable, Sequence

TIMESTAMP_URL = "http://timestamp.acs.microsoft.com"
DLIB = pathlib.PurePath("bin", "x64", "Azure.CodeSigning.Dlib.dll")


class WindowsSigningError(ValueError):
    pass


def metadata(endpoint: str, account: str, profile: str) -> dict[str, str]:
    for label, value in (("endpoint", endpoint), ("account", account), ("certificate profile", profile)):
        if not value.strip():
            raise WindowsSigningError(f"the Artifact Signing {label} is empty")
    if not endpoint.startswith("https://"):
        raise WindowsSigningError(f"the Artifact Signing endpoint {endpoint!r} is not an https URL")
    return {"Endpoint": endpoint, "CodeSigningAccountName": account, "CertificateProfileName": profile}


def tauri_overlay(signtool: str, dlib: str, metadata_path: str) -> dict:
    return {
        "bundle": {
            "createUpdaterArtifacts": False,
            "windows": {
                "signCommand": {
                    "cmd": signtool,
                    "args": [
                        "sign",
                        "/v",
                        "/fd",
                        "SHA256",
                        "/tr",
                        TIMESTAMP_URL,
                        "/td",
                        "SHA256",
                        "/dlib",
                        dlib,
                        "/dmdf",
                        metadata_path,
                        "%1",
                    ],
                }
            },
        }
    }


def extract_client(package: pathlib.Path, destination: pathlib.Path) -> pathlib.Path:
    """Unpacks the x64 client from the Microsoft.ArtifactSigning.Client package."""
    prefix = DLIB.parent.as_posix() + "/"
    try:
        with zipfile.ZipFile(package) as archive:
            members = [name for name in archive.namelist() if name.startswith(prefix) and not name.endswith("/")]
            for name in members:
                target = destination.joinpath(*name.split("/"))
                if destination.resolve() not in target.resolve().parents:
                    raise WindowsSigningError(f"{package} holds an entry outside its folder: {name}")
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(archive.read(name))
    except (OSError, zipfile.BadZipFile) as error:
        raise WindowsSigningError(f"cannot unpack {package}: {error}") from error
    dlib = destination / DLIB
    if not dlib.is_file():
        raise WindowsSigningError(f"{package} has no {DLIB.as_posix()}")
    return dlib


def write_config(
    client_package: pathlib.Path,
    signtool: str,
    endpoint: str,
    account: str,
    profile: str,
    out_dir: pathlib.Path,
) -> pathlib.Path:
    if not signtool.strip():
        raise WindowsSigningError("no signtool path")
    fields = metadata(endpoint, account, profile)
    out_dir.mkdir(parents=True, exist_ok=True)
    dlib = extract_client(client_package, out_dir / "client")
    metadata_path = out_dir / "metadata.json"
    metadata_path.write_text(json.dumps(fields, indent=2) + "\n")
    config_path = out_dir / "tauri.windows-signing.conf.json"
    config_path.write_text(
        json.dumps(tauri_overlay(signtool, str(dlib.resolve()), str(metadata_path.resolve())), indent=2) + "\n"
    )
    return config_path


def signing_command(config_path: pathlib.Path, file: pathlib.Path) -> list[str]:
    """The overlay's sign command for one file, with %1 replaced as the bundler replaces it."""
    try:
        command = json.loads(config_path.read_text())["bundle"]["windows"]["signCommand"]
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise WindowsSigningError(f"{config_path} holds no bundle.windows.signCommand: {error}") from error
    cmd = command.get("cmd") if isinstance(command, dict) else None
    args = command.get("args") if isinstance(command, dict) else None
    if not isinstance(cmd, str) or not cmd.strip():
        raise WindowsSigningError(f"the sign command in {config_path} names no program")
    if not isinstance(args, list) or not all(isinstance(arg, str) for arg in args):
        raise WindowsSigningError(f"the sign command in {config_path} has no list of string arguments")
    if "%1" not in args:
        raise WindowsSigningError(f"the sign command in {config_path} has no %1 argument for the file")
    if not file.is_file():
        raise WindowsSigningError(f"no file to sign at {file}")
    target = str(file.resolve())
    return [cmd, *(target if arg == "%1" else arg for arg in args)]


def sign_file(
    config_path: pathlib.Path,
    file: pathlib.Path,
    run: Callable[[Sequence[str]], subprocess.CompletedProcess] = lambda command: subprocess.run(command, check=False),
) -> None:
    command = signing_command(config_path, file)
    code = run(command).returncode
    if code != 0:
        raise WindowsSigningError(f"{command[0]} exited with {code} while signing {file}")


def sign_main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="windows_signing.py sign", description="Signs one file with the overlay's sign command.")
    parser.add_argument("config", type=pathlib.Path)
    parser.add_argument("file", type=pathlib.Path)
    args = parser.parse_args(argv)
    try:
        sign_file(args.config, args.file)
    except WindowsSigningError as error:
        print(f"windows_signing: {error}", file=sys.stderr)
        return 1
    return 0


def main(argv: list[str] | None = None) -> int:
    arguments = sys.argv[1:] if argv is None else argv
    if arguments[:1] == ["sign"]:
        return sign_main(arguments[1:])
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--client-package", required=True, type=pathlib.Path)
    parser.add_argument("--signtool", required=True)
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--account", required=True)
    parser.add_argument("--profile", required=True)
    parser.add_argument("--out-dir", required=True, type=pathlib.Path)
    args = parser.parse_args(arguments)
    try:
        path = write_config(args.client_package, args.signtool, args.endpoint, args.account, args.profile, args.out_dir)
    except WindowsSigningError as error:
        print(f"windows_signing: {error}", file=sys.stderr)
        return 1
    print(path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
