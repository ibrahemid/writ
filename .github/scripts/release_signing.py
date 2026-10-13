#!/usr/bin/env python3
"""Signs Writ's updater payloads and release checksums with the updater key, and checks them.

The key is the one `tauri signer generate` writes: TAURI_SIGNING_PRIVATE_KEY
holds the base64 of a minisign secret key (or a path to that file), and
TAURI_SIGNING_PRIVATE_KEY_PASSWORD its password. Signing runs minisign. Each
<payload>.sig is the base64 of a minisign signature whose trusted comment names
the file and the version, the format `tauri signer sign --app-version` writes
and tauri-plugin-updater reads. SHA256SUMS.txt.minisig is a plain minisign
signature that `minisign -Vm SHA256SUMS.txt -P <key>` checks.

Run from the repository root:
    release_signing.py sign-payloads --version V FILE...
    release_signing.py sign-sums --version V --dir DIR
    release_signing.py verify-payloads --version V --pubkey-from CONF FILE...
    release_signing.py verify-release --version V --tag T --repo R --dir DIR --pubkey-from CONF
"""
from __future__ import annotations

import argparse
import base64
import binascii
import contextlib
import dataclasses
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time
from collections.abc import Iterator, Mapping

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import release_assets  # noqa: E402

UNTRUSTED_COMMENT = "signature from tauri secret key"
KEY_ENV = "TAURI_SIGNING_PRIVATE_KEY"
PASSWORD_ENV = "TAURI_SIGNING_PRIVATE_KEY_PASSWORD"
MINISIGN_TIMEOUT_SECONDS = 300

# latest.json platform key -> the payload it serves, by the suffix of its name.
MANIFEST_PLATFORMS = {
    "darwin-aarch64": ".app.tar.gz",
    "darwin-x86_64": ".app.tar.gz",
    "windows-x86_64": ".msi",
    "linux-x86_64": ".AppImage",
    "linux-x86_64-deb": ".deb",
}


class SigningError(RuntimeError):
    pass


@dataclasses.dataclass(frozen=True)
class PublicKey:
    key_id: str
    line: str


def key_id_from_bytes(raw: bytes) -> str:
    return f"{int.from_bytes(raw[2:10], 'little'):016X}"


def public_key_from_text(text: str) -> PublicKey:
    """A minisign public key file: an untrusted comment line, then the key."""
    lines = [line.strip() for line in text.strip().splitlines() if line.strip()]
    if len(lines) != 2 or not lines[0].startswith("untrusted comment:"):
        raise SigningError("the public key is not a minisign public key file")
    try:
        raw = base64.b64decode(lines[1], validate=True)
    except binascii.Error as error:
        raise SigningError("the public key line is not base64") from error
    if len(raw) != 42 or raw[:2] != b"Ed":
        raise SigningError("the public key is not an Ed25519 minisign key")
    return PublicKey(key_id=key_id_from_bytes(raw), line=lines[1])


def public_key_from_config(config_path: pathlib.Path) -> PublicKey:
    """The updater key a tauri.conf.json embeds under plugins.updater.pubkey."""
    try:
        config = json.loads(config_path.read_text())
        encoded = config["plugins"]["updater"]["pubkey"]
    except (OSError, ValueError, KeyError, TypeError) as error:
        raise SigningError(f"{config_path} has no plugins.updater.pubkey: {error}") from error
    try:
        text = base64.b64decode(encoded, validate=True).decode("utf-8")
    except (binascii.Error, UnicodeDecodeError) as error:
        raise SigningError(f"plugins.updater.pubkey in {config_path} is not a base64 minisign key") from error
    return public_key_from_text(text)


def signature_key_id(minisig_text: str) -> str:
    lines = minisig_text.splitlines()
    if len(lines) < 4 or not lines[0].startswith("untrusted comment:") or not lines[2].startswith("trusted comment: "):
        raise SigningError("not a minisign signature")
    try:
        raw = base64.b64decode(lines[1].strip(), validate=True)
    except binascii.Error as error:
        raise SigningError("the signature line is not base64") from error
    if len(raw) != 74:
        raise SigningError("the signature line has the wrong length")
    return key_id_from_bytes(raw)


def trusted_comment(file_name: str, version: str, timestamp: int | None = None) -> str:
    for label, value in (("file name", file_name), ("version", version)):
        if any(ch in value for ch in "\t\r\n"):
            raise SigningError(f"the {label} {value!r} cannot go in a trusted comment")
    stamp = int(time.time()) if timestamp is None else timestamp
    return f"timestamp:{stamp}\tfile:{file_name}\tversion:{version}"


def parse_trusted_comment(comment: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    for field in comment.strip().split("\t"):
        key, separator, value = field.partition(":")
        if separator:
            fields[key] = value
    return fields


def tauri_signature(minisig: bytes) -> str:
    """The .sig format Tauri writes: the minisign signature file in base64."""
    return base64.b64encode(minisig).decode("ascii")


def minisig_from_tauri_signature(encoded: str) -> str:
    try:
        return base64.b64decode(encoded.strip(), validate=True).decode("utf-8")
    except (binascii.Error, UnicodeDecodeError) as error:
        raise SigningError("an updater signature is not base64 of a minisign signature") from error


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sums_text(directory: pathlib.Path, names: list[str]) -> str:
    return "".join(f"{sha256_file(directory / name)}  {name}\n" for name in sorted(names))


def parse_sums(text: str) -> dict[str, str]:
    entries: dict[str, str] = {}
    for number, line in enumerate(text.splitlines(), start=1):
        digest, separator, name = line.partition("  ")
        if not separator or len(digest) != 64 or not name or name in entries:
            raise SigningError(f"{release_assets.SUMS} line {number} is not '<sha256>  <name>': {line!r}")
        entries[name] = digest
    return entries


def names_a_file(text: str) -> bool:
    try:
        return pathlib.Path(text).is_file()
    except (OSError, ValueError):
        return False


@contextlib.contextmanager
def secret_key_file(env: Mapping[str, str]) -> Iterator[pathlib.Path]:
    raw = env.get(KEY_ENV, "")
    if not raw.strip():
        raise SigningError(f"{KEY_ENV} is empty; the signing job reads it from the release environment")
    if names_a_file(raw.strip()):
        raw = pathlib.Path(raw.strip()).read_text()
    try:
        decoded = base64.b64decode(raw.strip(), validate=True)
    except binascii.Error as error:
        raise SigningError(
            f"{KEY_ENV} is not base64; store the contents of the .key file `tauri signer generate` wrote"
        ) from error
    if not decoded.startswith(b"untrusted comment:"):
        raise SigningError(f"{KEY_ENV} does not decode to a minisign secret key")
    directory = pathlib.Path(tempfile.mkdtemp(prefix="writ-updater-key-"))
    try:
        path = directory / "updater.key"
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(decoded)
        yield path
    finally:
        shutil.rmtree(directory, ignore_errors=True)


def password_line(env: Mapping[str, str]) -> str:
    password = env.get(PASSWORD_ENV, "")
    if "\n" in password or "\r" in password:
        raise SigningError(f"{PASSWORD_ENV} holds a line break")
    return password + "\n"


def failure(result: subprocess.CompletedProcess[str]) -> str:
    if result.returncode < 0:
        return f"minisign was killed by signal {-result.returncode}"
    text = result.stderr + result.stdout
    lines = [line for line in text.strip().splitlines() if line.strip() and not line.startswith("Password:")]
    return lines[-1] if lines else f"minisign exited with status {result.returncode} and no output"


def minisign_sign(
    minisign: str, key: pathlib.Path, password: str, file: pathlib.Path, output: pathlib.Path, comment: str
) -> None:
    result = subprocess.run(
        [minisign, "-S", "-s", str(key), "-m", str(file), "-x", str(output), "-t", comment, "-c", UNTRUSTED_COMMENT],
        input=password,
        capture_output=True,
        text=True,
        check=False,
        timeout=MINISIGN_TIMEOUT_SECONDS,
    )
    if result.returncode != 0:
        raise SigningError(f"minisign could not sign {file.name}: {failure(result)}")


def minisign_verify(minisign: str, key: PublicKey, file: pathlib.Path, minisig_text: str) -> dict[str, str]:
    signed_by = signature_key_id(minisig_text)
    if signed_by != key.key_id:
        raise SigningError(
            f"{file.name} is signed with key {signed_by}, and installed copies trust key {key.key_id}"
        )
    with tempfile.TemporaryDirectory(prefix="writ-verify-") as scratch:
        signature = pathlib.Path(scratch) / f"{file.name}.minisig"
        signature.write_text(minisig_text)
        result = subprocess.run(
            [minisign, "-V", "-Q", "-P", key.line, "-m", str(file), "-x", str(signature)],
            capture_output=True,
            text=True,
            check=False,
            timeout=MINISIGN_TIMEOUT_SECONDS,
        )
    if result.returncode != 0:
        raise SigningError(f"{file.name} does not verify against key {key.key_id}: {failure(result)}")
    return parse_trusted_comment(result.stdout)


def require_comment(file: pathlib.Path, fields: dict[str, str], version: str) -> None:
    if fields.get("file") != file.name:
        raise SigningError(f"the signature of {file.name} names the file {fields.get('file')!r}")
    if fields.get("version") != version:
        raise SigningError(
            f"the signature of {file.name} is bound to version {fields.get('version')!r}, not {version}"
        )


def sign_payloads(files: list[pathlib.Path], version: str, minisign: str, env: Mapping[str, str]) -> None:
    release_assets.require_version(version)
    for file in files:
        if not file.is_file():
            raise SigningError(f"no payload at {file}")
    password = password_line(env)
    with secret_key_file(env) as key, tempfile.TemporaryDirectory(prefix="writ-sign-") as scratch:
        for file in files:
            minisig = pathlib.Path(scratch) / f"{file.name}.minisig"
            minisign_sign(minisign, key, password, file, minisig, trusted_comment(file.name, version))
            file.with_name(f"{file.name}.sig").write_text(tauri_signature(minisig.read_bytes()))
            print(f"signed {file.name}")


def sign_sums(directory: pathlib.Path, version: str, minisign: str, env: Mapping[str, str]) -> None:
    allowed = set(release_assets.asset_set(version, "published"))
    unexpected = sorted(entry.name for entry in directory.iterdir() if entry.name not in allowed)
    if unexpected:
        raise SigningError(f"{directory} holds files that are not part of the release: {', '.join(unexpected)}")
    names = release_assets.asset_set(version, "checksummed")
    missing = [name for name in names if not (directory / name).is_file()]
    if missing:
        raise SigningError(f"cannot write {release_assets.SUMS} without {', '.join(missing)}")
    sums = directory / release_assets.SUMS
    sums.write_text(sums_text(directory, names))
    password = password_line(env)
    with secret_key_file(env) as key:
        minisign_sign(
            minisign,
            key,
            password,
            sums,
            directory / release_assets.SUMS_SIGNATURE,
            trusted_comment(sums.name, version),
        )
    print(f"wrote {release_assets.SUMS} over {len(names)} files and signed it")


def verify_payloads(files: list[pathlib.Path], version: str, key: PublicKey, minisign: str) -> None:
    for file in files:
        signature = file.with_name(f"{file.name}.sig")
        if not file.is_file() or not signature.is_file():
            raise SigningError(f"{file.name} and {signature.name} must both exist")
        minisig = minisig_from_tauri_signature(signature.read_text())
        require_comment(file, minisign_verify(minisign, key, file, minisig), version)
        print(f"{file.name}: signed by key {key.key_id} for version {version}")


def verify_manifest(directory: pathlib.Path, version: str, tag: str, repo: str) -> None:
    try:
        manifest = json.loads((directory / release_assets.UPDATER_MANIFEST).read_text())
    except (OSError, ValueError) as error:
        raise SigningError(f"{release_assets.UPDATER_MANIFEST} is not readable JSON: {error}") from error
    if manifest.get("version") != version:
        raise SigningError(f"{release_assets.UPDATER_MANIFEST} announces {manifest.get('version')!r}, not {version}")
    platforms = manifest.get("platforms")
    if not isinstance(platforms, dict) or set(platforms) != set(MANIFEST_PLATFORMS):
        raise SigningError(
            f"{release_assets.UPDATER_MANIFEST} platforms are {sorted(platforms or {})}, "
            f"expected {sorted(MANIFEST_PLATFORMS)}"
        )
    payloads = release_assets.updater_payloads(version)
    for platform, suffix in MANIFEST_PLATFORMS.items():
        [name] = [payload for payload in payloads if payload.endswith(suffix)]
        entry = platforms[platform]
        url = f"https://github.com/{repo}/releases/download/{tag}/{name}"
        if entry.get("url") != url:
            raise SigningError(f"{platform} points at {entry.get('url')!r}, not {url}")
        if entry.get("signature") != (directory / f"{name}.sig").read_text().strip():
            raise SigningError(f"{platform} does not carry the signature of {name}")
    print(f"{release_assets.UPDATER_MANIFEST}: every platform serves a signed {version} payload of this release")


def verify_release(directory: pathlib.Path, version: str, tag: str, repo: str, key: PublicKey, minisign: str) -> None:
    difference = release_assets.compare_dir(directory, release_assets.asset_set(version, "published"))
    if difference.missing or difference.unexpected:
        raise SigningError(f"{directory} is not the published set: {'; '.join(difference.problems())}")
    verify_payloads([directory / name for name in release_assets.updater_payloads(version)], version, key, minisign)

    sums_path = directory / release_assets.SUMS
    listed = parse_sums(sums_path.read_text())
    expected = release_assets.asset_set(version, "checksummed")
    if sorted(listed) != expected:
        raise SigningError(f"{release_assets.SUMS} lists {sorted(listed)}, expected {expected}")
    for name, digest in listed.items():
        if sha256_file(directory / name) != digest:
            raise SigningError(f"{name} does not match its line in {release_assets.SUMS}")
    sums_signature = (directory / release_assets.SUMS_SIGNATURE).read_text()
    require_comment(sums_path, minisign_verify(minisign, key, sums_path, sums_signature), version)
    print(f"{release_assets.SUMS}: {len(listed)} files, signed by key {key.key_id} for version {version}")

    verify_manifest(directory, version, tag, repo)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--minisign", default=os.environ.get("MINISIGN", "minisign"))
    sub = parser.add_subparsers(dest="command", required=True)

    sign_cmd = sub.add_parser("sign-payloads", help="write <file>.sig for each updater payload")
    sign_cmd.add_argument("--version", required=True)
    sign_cmd.add_argument("files", nargs="+", type=pathlib.Path)

    sums_cmd = sub.add_parser("sign-sums", help="write and sign SHA256SUMS.txt over the checksummed set")
    sums_cmd.add_argument("--version", required=True)
    sums_cmd.add_argument("--dir", required=True, type=pathlib.Path)

    verify_cmd = sub.add_parser("verify-payloads", help="check updater signatures against a config's key")
    verify_cmd.add_argument("--version", required=True)
    verify_cmd.add_argument("--pubkey-from", required=True, type=pathlib.Path)
    verify_cmd.add_argument("files", nargs="+", type=pathlib.Path)

    release_cmd = sub.add_parser("verify-release", help="check a release directory before it is published")
    release_cmd.add_argument("--version", required=True)
    release_cmd.add_argument("--tag", required=True)
    release_cmd.add_argument("--repo", required=True)
    release_cmd.add_argument("--dir", required=True, type=pathlib.Path)
    release_cmd.add_argument("--pubkey-from", required=True, type=pathlib.Path)

    args = parser.parse_args(argv)
    if shutil.which(args.minisign) is None:
        print(f"release_signing: minisign not found at {args.minisign!r}", file=sys.stderr)
        return 1
    try:
        if args.command == "sign-payloads":
            sign_payloads(args.files, args.version, args.minisign, os.environ)
        elif args.command == "sign-sums":
            sign_sums(args.dir, args.version, args.minisign, os.environ)
        elif args.command == "verify-payloads":
            verify_payloads(args.files, args.version, public_key_from_config(args.pubkey_from), args.minisign)
        elif args.command == "verify-release":
            verify_release(
                args.dir, args.version, args.tag, args.repo, public_key_from_config(args.pubkey_from), args.minisign
            )
    except (SigningError, release_assets.ReleaseAssetError) as error:
        print(f"release_signing: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
