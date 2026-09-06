#!/usr/bin/env python3
"""Generate THIRD-PARTY-NOTICES.md from the dependencies shipped in the installers.

Rust licences come from cargo-about, chosen over cargo-bundle-licenses because it
resolves the dependency graph per target triple and per activated feature, so the
listing matches what is actually linked into a release build. Install it with:

    cargo install cargo-about --version 0.9.1 --locked --features cli

The version is pinned because cargo-about decides what to fall back to when a
crate ships no licence file, and this script fails rather than emit a fallback
that still has the SPDX template's blanks in it.

The triples, the accepted licence set, and the dev-dependency exclusion live in
about.toml; licences/notices.toml carries the texts for crates whose published
package leaves the licence file out. npm licences come from `pnpm licenses list
--prod` against the root package.json, whose dependencies are the ones bundled by
Vite into the app; the site under site/ has its own tree and does not ship.
Fonts bundled as files rather than packages are declared in the same
licences/notices.toml, under [[bundled]].

Usage:
    scripts/gen-third-party-notices.py            write THIRD-PARTY-NOTICES.md
    scripts/gen-third-party-notices.py --check    exit 1 if the file is stale
"""

from __future__ import annotations

import argparse
import difflib
import glob
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUTPUT_PATH = ROOT / "THIRD-PARTY-NOTICES.md"
ABOUT_CONFIG = ROOT / "about.toml"
NOTICES_DIR = ROOT / "licences"
NOTICES_CONFIG = NOTICES_DIR / "notices.toml"

CARGO_ABOUT_VERSION = "0.9.1"
CARGO_ABOUT_INSTALL = (
    f"cargo install cargo-about --version {CARGO_ABOUT_VERSION} --locked --features cli"
)

# File name prefixes npm packages use for the licence they ship.
LICENSE_FILE_PREFIXES = ("licence", "license", "copying")

# `.spdx` is a machine-readable declaration, not the licence text.
DECLARATION_SUFFIXES = (".spdx",)

# The blanks an SPDX licence template is published with. cargo-about fills a
# crate's entry with the template when it finds no licence file, so any of these
# reaching the output is a crate whose real notice was never found.
TEMPLATE_BLANKS = (
    "<year>",
    "<copyright holders>",
    "<copyright holder>",
    "<owner>",
    "<organization>",
    "<name of author>",
)


class GenerationError(RuntimeError):
    """A prerequisite is missing or an upstream tool failed."""


def run(cmd: list[str], **kwargs: object) -> str:
    """Run a command in the repo root and return stdout, or raise with its stderr."""
    try:
        proc = subprocess.run(
            cmd,
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
            **kwargs,  # type: ignore[arg-type]
        )
    except FileNotFoundError as exc:
        raise GenerationError(f"{cmd[0]} is not on PATH") from exc
    except subprocess.CalledProcessError as exc:
        raise GenerationError(
            f"{' '.join(cmd)} failed with exit code {exc.returncode}:\n{exc.stderr.strip()}"
        ) from exc
    return proc.stdout


def normalize_text(raw: str) -> str:
    """Verbatim licence text with platform line endings and trailing blanks removed.

    Only the line endings change: dedup keys and diffs must not depend on whether
    a crate shipped its licence with CRLF.
    """
    return raw.replace("\r\n", "\n").replace("\r", "\n").strip("\n")


def fence_for(text: str) -> str:
    """Shortest backtick fence that cannot be closed by a line inside `text`."""
    longest = 0
    for line in text.split("\n"):
        stripped = line.lstrip()
        run_length = len(stripped) - len(stripped.lstrip("`"))
        longest = max(longest, run_length)
    return "`" * max(3, longest + 1)


def workspace_member_names() -> set[str]:
    """Names of the crates in this repo, which are not third-party."""
    meta = json.loads(run(["cargo", "metadata", "--format-version", "1", "--no-deps"]))
    return {pkg["name"] for pkg in meta["packages"]}


def is_template(text: str) -> bool:
    """Whether a licence text still carries an SPDX template's blanks."""
    return any(blank in text for blank in TEMPLATE_BLANKS)


def strip_template_blanks(text: str) -> str:
    """The licence text with the lines that were never filled in taken out."""
    kept: list[str] = []
    for line in text.split("\n"):
        if any(blank in line for blank in TEMPLATE_BLANKS):
            continue
        if not line.strip() and kept and not kept[-1].strip():
            continue
        kept.append(line)
    return "\n".join(kept).strip("\n")


def license_file_key(name: str) -> str:
    """A licence file name reduced to `<kind>` or `<kind>-<licence id>`.

    The same file is called `LICENSE_MIT`, `license-mit` and `LICENSE.MIT`
    across crates, and the extension carries nothing.
    """
    key = name.lower()
    for suffix in (".txt", ".md"):
        if key.endswith(suffix):
            key = key[: -len(suffix)]
    return re.sub(r"[._\s-]+", "-", key)


def read_crate_license(crate_dir: pathlib.Path | None, licence: str) -> str | None:
    """The text a crate ships for `licence`, or None when it ships none.

    cargo-about looks for a fixed set of file names, so a crate shipping
    `LICENSE_MIT` (tauri) or `license-mit` (the windows crates) is reported as
    having no licence file at all. A name that pairs the licence identifier with
    the file wins; failing that an unqualified `LICENSE` is the crate's notice
    for everything it declares. A name qualified with a different licence never
    matches: brotli ships `LICENSE.MIT` and nothing for the BSD half of
    "BSD-3-Clause AND MIT".
    """
    if crate_dir is None or not crate_dir.is_dir():
        return None
    wanted = {
        f"{prefix}-{licence.lower().replace(' ', '-')}" for prefix in LICENSE_FILE_PREFIXES
    }
    qualified: str | None = None
    unqualified: str | None = None
    for path in sorted(crate_dir.iterdir()):
        if not path.is_file() or path.name.lower().endswith(DECLARATION_SUFFIXES):
            continue
        key = license_file_key(path.name)
        if key not in wanted and key not in LICENSE_FILE_PREFIXES:
            continue
        text = normalize_text(path.read_text(errors="replace"))
        if not text:
            continue
        if key in wanted:
            qualified = qualified or text
        else:
            unqualified = unqualified or text
    return qualified or unqualified


def load_vendored_notices() -> tuple[dict[tuple[str, str], str], set[tuple[str, str]]]:
    """Licence texts kept in-repo, and the crates that publish no notice at all.

    Both are keyed by crate name and licence identifier: a crate can need a text
    for one half of its expression and have shipped the other.
    """
    if not NOTICES_CONFIG.is_file():
        raise GenerationError(f"missing {NOTICES_CONFIG.relative_to(ROOT)}")
    with NOTICES_CONFIG.open("rb") as handle:
        config = tomllib.load(handle)

    where = NOTICES_CONFIG.relative_to(ROOT)
    texts: dict[tuple[str, str], str] = {}
    unattributed: set[tuple[str, str]] = set()

    def keys_of(entry: dict[str, object], table: str) -> list[tuple[str, str]]:
        crates = entry.get("crates")
        licence = entry.get("licence")
        if not crates or not isinstance(licence, str):
            raise GenerationError(f"{where}: a [[{table}]] entry needs `crates` and `licence`")
        claimed = []
        for name in crates:  # type: ignore[union-attr]
            key = (str(name), licence)
            if key in texts or key in unattributed:
                raise GenerationError(f"{where}: {key[0]} is listed twice for {licence}")
            claimed.append(key)
        return claimed

    for entry in config.get("notice", []):
        file_name = entry.get("file")
        if not isinstance(file_name, str):
            raise GenerationError(f"{where}: a [[notice]] entry needs `file`")
        if not entry.get("sources"):
            raise GenerationError(f"{where}: {file_name} records no `sources`")
        path = NOTICES_DIR / file_name
        if not path.is_file():
            raise GenerationError(f"{where}: {file_name} does not exist")
        text = normalize_text(path.read_text())
        if not text:
            raise GenerationError(f"{where}: {file_name} is empty")
        if is_template(text):
            raise GenerationError(f"{where}: {file_name} is an unfilled licence template")
        for key in keys_of(entry, "notice"):
            texts[key] = text

    for entry in config.get("unattributed", []):
        for key in keys_of(entry, "unattributed"):
            unattributed.add(key)

    return texts, unattributed


def load_bundled_assets() -> list[dict[str, object]]:
    """Files shipped inside the app that are neither a crate nor an npm package.

    A font file or a vendored library has no package manager to declare its
    licence, so the entry names the files it covers and the generator fails
    when one of them is gone.
    """
    with NOTICES_CONFIG.open("rb") as handle:
        config = tomllib.load(handle)

    where = NOTICES_CONFIG.relative_to(ROOT)
    assets: list[dict[str, object]] = []
    for entry in config.get("bundled", []):
        name = entry.get("name")
        licence = entry.get("licence")
        file_name = entry.get("file")
        files = entry.get("files")
        if not isinstance(name, str) or not isinstance(licence, str):
            raise GenerationError(f"{where}: a [[bundled]] entry needs `name` and `licence`")
        if not isinstance(file_name, str):
            raise GenerationError(f"{where}: {name} needs `file`")
        if not files:
            raise GenerationError(f"{where}: {name} records no `files`")
        if not entry.get("sources"):
            raise GenerationError(f"{where}: {name} records no `sources`")
        path = NOTICES_DIR / file_name
        if not path.is_file():
            raise GenerationError(f"{where}: {file_name} does not exist")
        text = normalize_text(path.read_text())
        if not text:
            raise GenerationError(f"{where}: {file_name} is empty")
        if is_template(text):
            raise GenerationError(f"{where}: {file_name} is an unfilled licence template")
        shipped = []
        for relative in files:  # type: ignore[union-attr]
            if not (ROOT / str(relative)).is_file():
                raise GenerationError(f"{where}: {name} names a missing file, {relative}")
            shipped.append(str(relative))
        assets.append(
            {"name": name, "id": licence, "text": text, "files": sorted(shipped)}
        )
    assets.sort(key=lambda asset: str(asset["name"]))
    return assets


def crate_source_dir(krate: dict[str, object]) -> pathlib.Path | None:
    """Where a crate's sources are unpacked, from its manifest path."""
    manifest = krate.get("manifest_path")
    return pathlib.Path(str(manifest)).parent if manifest else None


def crate_attribution(krate: dict[str, object]) -> tuple[str, str]:
    """The authors and repository a crate declares, for a crate with no notice."""
    authors = [
        re.sub(r"\s*<[^>]*>", "", str(author)).strip()
        for author in krate.get("authors") or []  # type: ignore[union-attr]
    ]
    return ", ".join(a for a in authors if a), str(krate.get("repository") or "")


def resolve_crate_text(
    licence: str,
    text: str,
    krate: dict[str, object],
    vendored: dict[tuple[str, str], str],
    unattributed: set[tuple[str, str]],
) -> tuple[str, str]:
    """One crate's licence text, and where it came from.

    cargo-about hands back the SPDX template for a crate it found no licence
    file in, blanks and all. Those crates are resolved here instead: from the
    file the crate does ship under a name cargo-about does not look for, then
    from licences/notices.toml.
    """
    if not is_template(text):
        return text, "about"
    key = (str(krate["name"]), licence)
    shipped = read_crate_license(crate_source_dir(krate), licence)
    if shipped is not None and not is_template(shipped):
        return shipped, "shipped"
    if key in vendored:
        return vendored[key], "vendored"
    if key in unattributed:
        return strip_template_blanks(text), "unattributed"
    raise GenerationError(
        f"no {licence} notice for {key[0]} {krate['version']}: it ships no licence file and "
        f"{NOTICES_CONFIG.relative_to(ROOT)} has no entry for it.\n"
        "Add the text from the crate's repository as a [[notice]], or record it as "
        "[[unattributed]] if the repository carries no copyright notice either."
    )


def collect_rust_groups() -> tuple[list[dict[str, object]], list[dict[str, object]], int]:
    """Deduplicated Rust licence texts, the crates that publish no copyright
    notice, and the number of crates covered."""
    if shutil.which("cargo-about") is None:
        raise GenerationError(
            f"cargo-about is not on PATH.\nInstall it with: {CARGO_ABOUT_INSTALL}"
        )
    if not ABOUT_CONFIG.is_file():
        raise GenerationError(f"missing {ABOUT_CONFIG.relative_to(ROOT)}")

    vendored, unattributed = load_vendored_notices()

    with tempfile.TemporaryDirectory() as tmp:
        out_path = pathlib.Path(tmp) / "about.json"
        run(
            [
                "cargo",
                "about",
                "generate",
                "--format",
                "json",
                "--workspace",
                "--locked",
                "--output-file",
                str(out_path),
            ]
        )
        data = json.loads(out_path.read_text())

    first_party = workspace_member_names()
    groups: dict[tuple[str, str], dict[str, object]] = {}
    unnamed: dict[tuple[str, str], dict[str, object]] = {}
    covered: set[tuple[str, str]] = set()
    claimed: set[tuple[str, str]] = set()

    for entry in data["licenses"]:
        licence = entry["id"]
        entry_text = normalize_text(entry["text"])
        for used in entry["used_by"]:
            krate = used["crate"]
            name = krate["name"]
            if name in first_party:
                continue
            crate = (name, krate["version"])
            covered.add(crate)
            text, source = resolve_crate_text(
                licence, entry_text, krate, vendored, unattributed
            )
            if source in ("vendored", "unattributed"):
                claimed.add((name, licence))
            no_notice = source == "unattributed"
            target = unnamed if no_notice else groups
            key = (licence, hashlib.sha256(text.encode("utf-8")).hexdigest())
            group = target.setdefault(
                key,
                {
                    "id": licence,
                    "name": entry["name"],
                    "text": text,
                    "crates": set(),
                    "attribution": {},
                },
            )
            group["crates"].add(crate)  # type: ignore[union-attr]
            if no_notice:
                group["attribution"][crate] = crate_attribution(krate)  # type: ignore[index]

    stale = sorted((set(vendored) | unattributed) - claimed)
    if stale:
        listed = ", ".join(f"{name} ({licence})" for name, licence in stale)
        raise GenerationError(
            f"{NOTICES_CONFIG.relative_to(ROOT)} has entries for crates that are no longer "
            f"resolved from it: {listed}.\nRemove them."
        )

    def order(source: dict[tuple[str, str], dict[str, object]]) -> list[dict[str, object]]:
        result = []
        for group in source.values():
            crates = sorted(group["crates"])  # type: ignore[arg-type]
            result.append({**group, "crates": crates})
        result.sort(key=lambda g: (g["id"], g["crates"][0], g["text"]))
        return result

    return order(groups), order(unnamed), len(covered)


def accepted_licences() -> list[str]:
    """Licence preference order, shared with the Rust side via about.toml."""
    with ABOUT_CONFIG.open("rb") as handle:
        return list(tomllib.load(handle)["accepted"])


def resolve_licence(declared: str, accepted: list[str]) -> str:
    """Reduce a declared expression to the one licence Writ ships under.

    A conjunction ("A AND B") means every licence applies, so it is left intact.
    A choice is resolved the way cargo-about resolves it: first match in
    `accepted`.
    """
    if " AND " in declared.upper():
        return declared
    candidates = [
        part.strip().strip("()")
        for part in declared.replace("/", " OR ").split(" OR ")
        if part.strip()
    ]
    for licence in accepted:
        if licence in candidates:
            return licence
    return declared


def license_files(package_dir: str) -> list[str]:
    """Paths of the licence files an npm package ships, sorted by file name."""
    found = []
    for path in sorted(glob.glob(os.path.join(package_dir, "*"))):
        name = os.path.basename(path).lower()
        if os.path.isfile(path) and name.startswith(LICENSE_FILE_PREFIXES):
            found.append(path)
    return found


def read_package_license(package_dir: str, licence: str) -> tuple[str, bool] | None:
    """Licence text an npm package ships, and whether it is only a declaration.

    Packages that dual-license ship one file per licence (`LICENSE_MIT`,
    `LICENSE_APACHE-2.0`); the one naming the resolved licence wins. A `.spdx`
    file states which licence applies without reproducing it, so it is used only
    when the package ships nothing else.
    """
    paths = license_files(package_dir)
    texts = []
    for path in paths:
        text = normalize_text(pathlib.Path(path).read_text(errors="replace"))
        if text:
            texts.append((path, text))

    wanted = licence.lower().replace(" ", "-")
    full = [
        (path, text)
        for path, text in texts
        if not path.lower().endswith(DECLARATION_SUFFIXES)
    ]
    for path, text in full:
        if wanted in os.path.basename(path).lower():
            return text, False
    if full:
        return full[0][1], False
    if texts:
        return texts[0][1], True
    return None


def collect_npm_groups() -> tuple[list[dict[str, object]], list[dict[str, object]], int, list[str]]:
    """Deduplicated npm licence texts, declaration-only groups, count, and gaps."""
    if not (ROOT / "node_modules").is_dir():
        raise GenerationError(
            "node_modules is missing.\nInstall it with: pnpm install --frozen-lockfile"
        )

    accepted = accepted_licences()
    data = json.loads(run(["pnpm", "licenses", "list", "--json", "--prod"]))
    groups: dict[tuple[str, str], dict[str, object]] = {}
    declarations: dict[tuple[str, str], dict[str, object]] = {}
    packages: set[tuple[str, str]] = set()
    without_text: list[str] = []

    for declared, entries in data.items():
        licence = resolve_licence(declared, accepted)
        for entry in entries:
            name = entry["name"]
            labelled = [(name, version) for version in sorted(entry.get("versions", []))]
            packages.update(labelled)
            result = None
            for package_dir in entry.get("paths", []):
                result = read_package_license(package_dir, licence)
                if result:
                    break
            if result is None:
                without_text.extend(f"{n} {v}" for n, v in labelled)
                continue
            text, declaration_only = result
            target = declarations if declaration_only else groups
            key = (licence, hashlib.sha256(text.encode("utf-8")).hexdigest())
            group = target.setdefault(
                key, {"id": licence, "text": text, "packages": set()}
            )
            group["packages"].update(labelled)  # type: ignore[union-attr]

    def order(source: dict[tuple[str, str], dict[str, object]]) -> list[dict[str, object]]:
        result = []
        for group in source.values():
            pkgs = sorted(group["packages"])  # type: ignore[arg-type]
            result.append({**group, "packages": pkgs})
        result.sort(key=lambda g: (g["id"], g["packages"][0], g["text"]))
        return result

    return order(groups), order(declarations), len(packages), sorted(without_text)


def render(
    rust_groups: list[dict[str, object]],
    rust_unnamed: list[dict[str, object]],
    rust_crate_count: int,
    npm_groups: list[dict[str, object]],
    npm_declarations: list[dict[str, object]],
    npm_package_count: int,
    npm_without_text: list[str],
    bundled_assets: list[dict[str, object]],
) -> str:
    lines: list[str] = []
    add = lines.append

    def block(group: dict[str, object], members: str) -> None:
        text = str(group["text"])
        add(f"### {group['id']}")
        add("")
        add(", ".join(f"{name} {version}" for name, version in group[members]))  # type: ignore[union-attr]
        add("")
        fence = fence_for(text)
        add(fence)
        add(text)
        add(fence)
        add("")

    add("# Third-party notices")
    add("")
    add(
        "Writ links in the Rust crates, bundles the npm packages, and ships the files "
        "listed below. Their licences require the copyright notice and "
        "permission text to be distributed with the binaries, so both are reproduced "
        "in full."
    )
    add("")
    bundled = len(bundled_assets)
    bundled_clause = (
        " 1 font family or library is shipped as files rather than as a package "
        "and is listed below."
        if bundled == 1
        else f" {bundled} font families and libraries are shipped as files rather "
        "than as packages and are listed below."
        if bundled
        else ""
    )
    add(
        f"{rust_crate_count} Rust crates are covered, resolved for every target a "
        "release is built for, and "
        f"{npm_package_count} npm packages, resolved from the production "
        "dependencies of the frontend bundle. Test and benchmark dependencies are "
        "excluded: they are not shipped." + bundled_clause
    )
    add("")
    add(
        "Generated by `scripts/gen-third-party-notices.py`. Edits are overwritten; "
        "`scripts/release-preflight.sh` fails when this file is out of date."
    )
    add("")

    add("## Summary")
    add("")
    add("| Licence | Rust crates | npm packages | Bundled files |")
    add("| --- | ---: | ---: | ---: |")
    counts: dict[str, list[int]] = {}
    for group in rust_groups + rust_unnamed:
        counts.setdefault(str(group["id"]), [0, 0, 0])[0] += len(group["crates"])  # type: ignore[arg-type]
    for group in npm_groups + npm_declarations:
        counts.setdefault(str(group["id"]), [0, 0, 0])[1] += len(group["packages"])  # type: ignore[arg-type]
    for asset in bundled_assets:
        counts.setdefault(str(asset["id"]), [0, 0, 0])[2] += 1
    for licence in sorted(counts):
        rust_count, npm_count, bundled_count = counts[licence]
        add(f"| {licence} | {rust_count or ''} | {npm_count or ''} | {bundled_count or ''} |")
    add("")

    add("## Rust crates")
    add("")
    add(
        "Each block is one distinct licence text and the crates it covers. Crates "
        "offering a choice of licences appear under the one Writ takes, in the "
        "order set by `about.toml`."
    )
    add("")
    for group in rust_groups:
        block(group, "crates")

    if rust_unnamed:
        add("## Rust crates that publish no copyright notice")
        add("")
        add(
            "These crates ship no licence file, and their source repositories carry "
            "no copyright notice at the commit each version was published from. The "
            "licence each one declares is reproduced with its unfilled copyright "
            "line removed, above the authors and repository the crate itself names."
        )
        add("")
        for group in rust_unnamed:
            add(f"### {group['id']}")
            add("")
            text = str(group["text"])
            fence = fence_for(text)
            add(fence)
            add(text)
            add(fence)
            add("")
            attribution: dict[tuple[str, str], tuple[str, str]] = group["attribution"]  # type: ignore[assignment]
            by_source: dict[tuple[str, str], list[str]] = {}
            for crate in group["crates"]:  # type: ignore[union-attr]
                by_source.setdefault(attribution[crate], []).append(
                    f"{crate[0]} {crate[1]}"
                )
            for (authors, repository), crates in sorted(
                by_source.items(), key=lambda item: item[1][0]
            ):
                trailer = ", ".join(part for part in (authors, repository) if part)
                add(f"- {', '.join(crates)}" + (f" ({trailer})" if trailer else ""))
            add("")

    add("## npm packages")
    add("")
    for group in npm_groups:
        block(group, "packages")

    if npm_declarations:
        add("## npm packages that ship a licence declaration")
        add("")
        add(
            "These packages state their copyright and the licence they grant in a "
            "machine-readable declaration rather than a copy of the licence. The "
            "declaration is reproduced verbatim; the text of the licence it names "
            "appears above."
        )
        add("")
        for group in npm_declarations:
            block(group, "packages")

    if npm_without_text:
        add("## npm packages with no licence file")
        add("")
        add(
            "These packages declare a licence in package.json and ship no licence "
            "file of any kind. The declared identifier is in the summary above."
        )
        add("")
        for package in npm_without_text:
            add(f"- {package}")
        add("")

    if bundled_assets:
        add("## Bundled files")
        add("")
        add(
            "Font files and libraries that ship inside the app rather than being "
            "installed by a package manager. Each block names the files it covers."
        )
        add("")
        for asset in bundled_assets:
            add(f"### {asset['name']} ({asset['id']})")
            add("")
            for relative in asset["files"]:  # type: ignore[union-attr]
                add(f"- `{relative}`")
            add("")
            text = str(asset["text"])
            fence = fence_for(text)
            add(fence)
            add(text)
            add(fence)
            add("")

    return "\n".join(lines).rstrip("\n") + "\n"


def generate() -> str:
    rust_groups, rust_unnamed, rust_crate_count = collect_rust_groups()
    npm_groups, npm_declarations, npm_package_count, npm_without_text = (
        collect_npm_groups()
    )
    bundled_assets = load_bundled_assets()
    document = render(
        rust_groups,
        rust_unnamed,
        rust_crate_count,
        npm_groups,
        npm_declarations,
        npm_package_count,
        npm_without_text,
        bundled_assets,
    )
    found = sorted({blank for blank in TEMPLATE_BLANKS if blank in document})
    if found:
        raise GenerationError(
            f"the notices still carry unfilled licence templates: {', '.join(found)}.\n"
            "A crate's real notice was not found and a placeholder was shipped in its "
            "place; resolve it in licences/notices.toml."
        )
    return document


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail when THIRD-PARTY-NOTICES.md differs from a fresh generation",
    )
    args = parser.parse_args()

    try:
        content = generate()
    except GenerationError as exc:
        print(f"third-party notices: {exc}", file=sys.stderr)
        return 1

    if args.check:
        if not OUTPUT_PATH.is_file():
            print(
                f"third-party notices: {OUTPUT_PATH.name} is missing; run "
                "scripts/gen-third-party-notices.py",
                file=sys.stderr,
            )
            return 1
        current = OUTPUT_PATH.read_text()
        if current != content:
            diff = difflib.unified_diff(
                current.splitlines(keepends=True),
                content.splitlines(keepends=True),
                fromfile=f"{OUTPUT_PATH.name} (committed)",
                tofile=f"{OUTPUT_PATH.name} (regenerated)",
            )
            sys.stderr.writelines(diff)
            print(
                f"\nthird-party notices: {OUTPUT_PATH.name} is out of date; run "
                "scripts/gen-third-party-notices.py and commit the result",
                file=sys.stderr,
            )
            return 1
        print(f"third-party notices: {OUTPUT_PATH.name} is up to date")
        return 0

    OUTPUT_PATH.write_text(content)
    print(f"third-party notices: wrote {OUTPUT_PATH.name} ({len(content)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
