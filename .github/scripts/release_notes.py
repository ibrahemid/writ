#!/usr/bin/env python3
"""Write the draft release notes for a tag, grouped by conventional-commit prefix.

Expected environment variables:
  TAG   the tag being released (v0.5.1, or v0.5.1-1 for a candidate)
  REPO  "<owner>/<repo>" GitHub repository identifier, for the compare link

The notes cover the commits since the previous tag. For a stable tag that is
the previous stable v*.*.* tag, skipping candidates, so a release cut after a
candidate still lists everything since the last release. A candidate takes the
previous v*.*.* tag of either kind.

The notes go to stdout and the range to stderr.
"""
from __future__ import annotations

import fnmatch
import os
import pathlib
import re
import subprocess
import sys

TAG_PATTERN = "v*.*.*"
PRERELEASE_PATTERN = "v*-*"
FEATURE = re.compile(r"^- feat(\(.+\))?: ")
FIX = re.compile(r"^- fix(\(.+\))?: ")


class ReleaseNotesError(RuntimeError):
    pass


def git(args: list[str], cwd: pathlib.Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args],
        cwd=cwd,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )


def is_prerelease(tag: str) -> bool:
    return "-" in tag


def previous_tag(tag: str, cwd: pathlib.Path) -> str | None:
    if git(["rev-parse", "--verify", "--quiet", f"{tag}^{{commit}}"], cwd).returncode != 0:
        raise ReleaseNotesError(f"tag {tag} does not name a commit")
    parent = f"{tag}^"
    if git(["rev-parse", "--verify", "--quiet", parent], cwd).returncode != 0:
        return None

    merged = git(["tag", "--merged", parent, "--list", TAG_PATTERN], cwd)
    if merged.returncode != 0:
        raise ReleaseNotesError(f"git tag --merged {parent} failed: {merged.stderr.strip()}")
    candidates = [
        name
        for name in merged.stdout.split()
        if is_prerelease(tag) or not fnmatch.fnmatchcase(name, PRERELEASE_PATTERN)
    ]
    if not candidates:
        return None

    args = ["describe", "--tags", "--abbrev=0", "--match", TAG_PATTERN]
    if not is_prerelease(tag):
        args += ["--exclude", PRERELEASE_PATTERN]
    described = git([*args, parent], cwd)
    if described.returncode != 0:
        raise ReleaseNotesError(f"git describe {parent} failed: {described.stderr.strip()}")
    return described.stdout.strip()


def commit_lines(revisions: str, cwd: pathlib.Path) -> list[str]:
    log = git(["log", "--no-merges", "--pretty=format:- %s (%h)", revisions, "--"], cwd)
    if log.returncode != 0:
        raise ReleaseNotesError(f"git log {revisions} failed: {log.stderr.strip()}")
    return log.stdout.splitlines()


def render_notes(
    lines: list[str], *, tag: str, previous: str | None, repository: str
) -> str:
    features = [line for line in lines if FEATURE.match(line)]
    fixes = [line for line in lines if FIX.match(line)]
    other = [line for line in lines if not (FEATURE.match(line) or FIX.match(line))]

    out = [
        "## What's changed",
        "",
        "### Features",
        *(features or ["- None"]),
        "",
        "",
        "### Fixes",
        *(fixes or ["- None"]),
        "",
        "",
        "### Other",
        *(other or ["- None"]),
        "",
    ]
    if previous:
        out += [
            "",
            f"**Full Changelog**: https://github.com/{repository}/compare/{previous}...{tag}",
        ]
    return "\n".join(out) + "\n"


def release_notes(tag: str, repository: str, cwd: pathlib.Path) -> str:
    previous = previous_tag(tag, cwd)
    if previous:
        revisions = f"{previous}..{tag}"
        print(f"Generating changelog for range {revisions}", file=sys.stderr)
    else:
        revisions = tag
        print(f"No previous tag found, listing every commit up to {tag}", file=sys.stderr)
    return render_notes(
        commit_lines(revisions, cwd), tag=tag, previous=previous, repository=repository
    )


def main() -> int:
    tag = os.environ.get("TAG", "")
    repository = os.environ.get("REPO", "")
    missing = [name for name, value in (("TAG", tag), ("REPO", repository)) if not value]
    if missing:
        print(f"Missing required env var: {', '.join(missing)}", file=sys.stderr)
        return 1
    try:
        notes = release_notes(tag, repository, pathlib.Path.cwd())
    except ReleaseNotesError as error:
        print(f"release_notes: {error}", file=sys.stderr)
        return 1
    sys.stdout.write(notes)
    return 0


if __name__ == "__main__":
    sys.exit(main())
