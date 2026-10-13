#!/usr/bin/env python3
"""Writes the Tauri config overlay that signs the Windows build through bundle.windows.signCommand.

The sign command is signtool with the Azure Artifact Signing dlib, which signs
with a certificate profile in an Artifact Signing account and authenticates
from AZURE_TENANT_ID, AZURE_CLIENT_ID and AZURE_CLIENT_SECRET in the environment.
The bundler runs it on the sidecar, on the main executable and on the MSI.

Run from the repository root:
    python .github/scripts/windows_signing.py --client-package client.nupkg --signtool PATH \\
        --endpoint URL --account NAME --profile NAME --out-dir DIR
"""
from __future__ import annotations

import argparse
import json
import pathlib
import sys
import zipfile

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


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--client-package", required=True, type=pathlib.Path)
    parser.add_argument("--signtool", required=True)
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--account", required=True)
    parser.add_argument("--profile", required=True)
    parser.add_argument("--out-dir", required=True, type=pathlib.Path)
    args = parser.parse_args(argv)
    try:
        path = write_config(args.client_package, args.signtool, args.endpoint, args.account, args.profile, args.out_dir)
    except WindowsSigningError as error:
        print(f"windows_signing: {error}", file=sys.stderr)
        return 1
    print(path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
