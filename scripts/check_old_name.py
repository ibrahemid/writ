#!/usr/bin/env python3
"""Fail on any spelling of the old product name outside the allowlist.

Scans the paths and text of the files git lists after masking the kept
tokens, and prints one line per hit as `path:line:column token`, then the
count. A hit in a path is reported at line 0.
"""

from __future__ import annotations

import argparse
import bisect
import dataclasses
import os
import pathlib
import re
import subprocess
import sys
from typing import Iterable, Sequence

DEFAULT_OLD_NAME = "Writ"
ALLOWLIST_PATH = pathlib.PurePosixPath("scripts/old-name-allowlist.txt")
APP_STAGE = "app"
CHANNELS_STAGE = "channels"
STAGES = (APP_STAGE, CHANNELS_STAGE)
NAME_RULE = re.compile(r"[A-Z][a-z0-9]+")
BINARY_SNIFF_BYTES = 8192
WALK_SKIP_DIRS = frozenset({".git", "node_modules", "target"})
TOKEN_CONTEXT_LIMIT = 80
NAME_START = r"(?:(?<![A-Za-z])|(?<=\\[nrt])|(?<=%[0-9A-Fa-f]{2})|(?<=(?<![A-Za-z])lib))"

KEPT_TOKEN_PATTERNS = (
    ("com.writ.editor/", r"com\.writ\.editor/writ-command"),
    ("com.writ.", r"(?<![A-Za-z0-9_-])com\.writ\."),
    ("COM.WRIT.", r"(?<![A-Za-z0-9_-])COM\.WRIT\."),
    ("writ-", r"(?<![A-Za-z0-9_-])writ-(?:light|dark|blue)(?![A-Za-z0-9_])"),
    ("WritBlue", r"(?<![A-Za-z0-9_])WritBlue(?![A-Za-z0-9_])"),
    ("ibrahemid.Writ", r"ibrahemid\.Writ(?![a-z])"),
    ("writ-bin", r"(?<![A-Za-z0-9_-])writ-bin(?![A-Za-z0-9_])"),
    ("homebrew-writ", r"homebrew-writ(?![A-Za-z0-9_])"),
    ("ibrahemid/writ", r"\bbrew\s+(?:tap|untap|trust)\s+ibrahemid/writ(?![A-Za-z0-9_-])"),
    ("ibrahemid/writ/", r"(?<![/A-Za-z0-9_.@:-])ibrahemid/writ(?=/[a-z])"),
    ("ab97cc4d", r"ab97cc4d-eea3-53ee-90d8-3c2767967676"),
)

APP_STAGE_PATTERNS = (
    ("ibrahemid/writ", r"ibrahemid/writ(?![A-Za-z0-9_-])"),
    ("ibrahemid", r"ibrahemid([\"']) / \1writ(?![A-Za-z0-9_-])"),
    ("writ.ibrahemid.com", r"(?<![A-Za-z0-9.-])writ\.ibrahemid\.com"),
)

ALLOW_LINE = re.compile(
    r"^(?P<glob>\S+)"
    r"(?:\s+:(?P<scope>path)|\s+:(?P<text_scope>from|lines|text)\s+/(?P<pattern>.+?)/)?"
    r"\s+--\s+(?P<reason>\S.*)$"
)
GLOB_WILDCARDS = frozenset("*?[")
PATH_SCOPES = (None, "path")
TOKEN_CHAR = re.compile(r"[A-Za-z0-9_-]")


class OldNameError(Exception):
    """Base class for the rename tooling's own failures."""


class InvalidNameError(OldNameError, ValueError):
    """A product name does not fit the case-preserving rename map."""


class AllowlistFormatError(OldNameError, ValueError):
    """The allowlist has a line or pattern that cannot be parsed."""


class FileListingError(OldNameError, RuntimeError):
    """The tree's files could not be listed."""


@dataclasses.dataclass(frozen=True)
class NameForms:
    display: str
    lower: str
    upper: str

    def get_replacement(self, matched: str, old: NameForms) -> str:
        if matched == old.display:
            return self.display
        if matched == old.lower:
            return self.lower
        if matched == old.upper:
            return self.upper
        raise InvalidNameError(f"{matched!r} is not a form of {old.display!r}")


@dataclasses.dataclass(frozen=True)
class AllowRule:
    glob: str
    scope: str | None
    pattern: re.Pattern[str] | None
    reason: str
    line_number: int
    path_regex: re.Pattern[str]
    literal_segments: frozenset[str]

    def matches_path(self, path: str) -> bool:
        return self.path_regex.fullmatch(path) is not None


@dataclasses.dataclass(frozen=True)
class Allowlist:
    rules: tuple[AllowRule, ...]

    def get_rules(self, *paths: str) -> list[AllowRule]:
        return [rule for rule in self.rules if any(rule.matches_path(path) for path in paths)]

    def is_whole_file(self, *paths: str) -> bool:
        return any(rule.scope is None for rule in self.get_rules(*paths))

    def keeps_path(self, *paths: str) -> bool:
        return any(rule.scope in PATH_SCOPES for rule in self.get_rules(*paths))


@dataclasses.dataclass(frozen=True)
class Hit:
    path: str
    line: int
    column: int
    token: str

    def format(self) -> str:
        return f"{self.path}:{self.line}:{self.column} {self.token}"


def get_name_forms(name: str) -> NameForms:
    if NAME_RULE.fullmatch(name) is None:
        raise InvalidNameError(
            f"{name!r} is not a product name: expected a capital letter followed by "
            "lowercase letters or digits"
        )
    return NameForms(display=name, lower=name.lower(), upper=name.upper())


def compile_name_pattern(forms: NameForms) -> re.Pattern[str]:
    display, lower, upper = (re.escape(form) for form in (forms.display, forms.lower, forms.upper))
    return re.compile(
        rf"{NAME_START}(?:{display}|{lower})(?![a-z])"
        rf"|(?<=[a-z0-9]){display}(?![a-z])"
        rf"|{NAME_START}{upper}(?![A-Z])"
    )


def compile_glob(glob: str) -> re.Pattern[str]:
    parts: list[str] = []
    index = 0
    while index < len(glob):
        char = glob[index]
        if glob.startswith("**", index):
            at_segment_start = index == 0 or glob[index - 1] == "/"
            if at_segment_start and glob.startswith("**/", index):
                parts.append("(?:[^/]+/)*")
                index += 3
            else:
                parts.append(".*")
                index += 2
        elif char == "*":
            parts.append("[^/]*")
            index += 1
        elif char == "?":
            parts.append("[^/]")
            index += 1
        elif char == "[":
            end = glob.find("]", index + 2)
            if end == -1:
                raise AllowlistFormatError(f"glob {glob!r} has an unclosed character class")
            body = glob[index + 1 : end]
            if body.startswith("!"):
                body = "^" + body[1:]
            parts.append("[" + body.replace("\\", "\\\\") + "]")
            index = end + 1
        else:
            parts.append(re.escape(char))
            index += 1
    return re.compile("".join(parts))


def parse_allowlist(text: str, source: str = "allowlist") -> Allowlist:
    rules: list[AllowRule] = []
    for line_number, raw in enumerate(text.split("\n"), start=1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        match = ALLOW_LINE.match(line)
        if match is None:
            raise AllowlistFormatError(
                f"{source}:{line_number}: expected `<glob> [:path | :from|:lines|:text /regex/] -- <reason>`, "
                f"got {line!r}"
            )
        pattern: re.Pattern[str] | None = None
        if match["pattern"] is not None:
            try:
                pattern = re.compile(match["pattern"])
            except re.error as error:
                raise AllowlistFormatError(
                    f"{source}:{line_number}: invalid pattern /{match['pattern']}/: {error}"
                ) from error
        glob = match["glob"]
        rules.append(
            AllowRule(
                glob=glob,
                scope=match["scope"] or match["text_scope"],
                pattern=pattern,
                reason=match["reason"],
                line_number=line_number,
                path_regex=compile_glob(glob),
                literal_segments=frozenset(
                    segment for segment in glob.split("/") if segment and not GLOB_WILDCARDS & set(segment)
                ),
            )
        )
    return Allowlist(rules=tuple(rules))


def load_allowlist(path: pathlib.Path) -> Allowlist:
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as error:
        raise AllowlistFormatError(f"cannot read the allowlist at {path}: {error}") from error
    return parse_allowlist(text, source=str(path))


def get_line_spans(text: str) -> Iterable[tuple[int, int, str]]:
    offset = 0
    for line in text.split("\n"):
        yield offset, offset + len(line), line.rstrip("\r")
        offset += len(line) + 1


def get_scope_spans(rule: AllowRule, text: str) -> list[tuple[int, int]]:
    if rule.scope is None:
        return [(0, len(text))]
    if rule.scope == "path" or rule.pattern is None:
        return []
    if rule.scope == "text":
        return [match.span() for match in rule.pattern.finditer(text) if match.end() > match.start()]
    spans: list[tuple[int, int]] = []
    for start, end, line in get_line_spans(text):
        if rule.pattern.search(line) is None:
            continue
        if rule.scope == "from":
            return [(start, len(text))]
        spans.append((start, end))
    return spans


def merge_spans(spans: Iterable[tuple[int, int]]) -> tuple[list[int], list[int]]:
    starts: list[int] = []
    ends: list[int] = []
    for start, end in sorted(spans):
        if starts and start <= ends[-1]:
            ends[-1] = max(ends[-1], end)
        else:
            starts.append(start)
            ends.append(end)
    return starts, ends


def is_protected(start: int, end: int, starts: Sequence[int], ends: Sequence[int]) -> bool:
    index = bisect.bisect_right(starts, start) - 1
    if index >= 0 and ends[index] > start:
        return True
    return index + 1 < len(starts) and starts[index + 1] < end


def get_pattern_spans(text: str, patterns: Iterable[tuple[str, re.Pattern[str]]]) -> list[tuple[int, int]]:
    spans: list[tuple[int, int]] = []
    for needle, pattern in patterns:
        if needle in text:
            spans.extend(match.span() for match in pattern.finditer(text))
    return spans


class NameScanner:
    """Finds the old name's unmasked spellings in paths and text."""

    def __init__(
        self,
        old: NameForms,
        stage: str,
        allowlist: Allowlist,
        files: Sequence[str] = (),
    ) -> None:
        if stage not in STAGES:
            raise OldNameError(f"stage must be one of {', '.join(STAGES)}, got {stage!r}")
        self.old = old
        self.stage = stage
        self.allowlist = allowlist
        self.name_pattern = compile_name_pattern(old)
        self.held_back_patterns = tuple((needle, re.compile(pattern)) for needle, pattern in APP_STAGE_PATTERNS)
        kept_patterns = tuple((needle, re.compile(pattern)) for needle, pattern in KEPT_TOKEN_PATTERNS)
        self.mask_patterns = kept_patterns + (self.held_back_patterns if stage == APP_STAGE else ())
        self.kept_path_patterns = self._compile_kept_path_patterns(files)

    def _compile_kept_path_patterns(self, files: Sequence[str]) -> tuple[tuple[str, re.Pattern[str]], ...]:
        literals: set[str] = set()
        for path in files:
            if not self.allowlist.keeps_path(path):
                continue
            components = path.split("/")
            for index in self.get_kept_component_indexes(path):
                component = components[index]
                if self.name_pattern.search(component) is None or index == 0:
                    continue
                parent = components[index - 1]
                literals.add(f"{parent}/{component}")
                if index == len(components) - 1:
                    stem, dot, _ = component.rpartition(".")
                    if dot and stem:
                        literals.add(f"{parent}/{stem}")
                        if stem not in (self.old.display, self.old.lower, self.old.upper):
                            literals.add(component)
        return tuple(
            (literal, re.compile(rf"(?<![A-Za-z0-9_.-]){re.escape(literal)}(?![A-Za-z0-9_-])"))
            for literal in sorted(literals, key=len, reverse=True)
        )

    def get_kept_component_indexes(self, path: str) -> set[int]:
        path_rules = [rule for rule in self.allowlist.get_rules(path) if rule.scope in PATH_SCOPES]
        if not path_rules:
            return set()
        literals = frozenset().union(*(rule.literal_segments for rule in path_rules))
        components = path.split("/")
        kept = {len(components) - 1}
        kept.update(index for index, component in enumerate(components) if component in literals)
        return kept

    def get_mask_spans(self, text: str) -> list[tuple[int, int]]:
        return get_pattern_spans(text, self.mask_patterns + self.kept_path_patterns)

    def get_held_back_spans(self, text: str) -> list[tuple[int, int]]:
        return get_pattern_spans(text, self.held_back_patterns)

    def find_unmasked(self, text: str, extra_spans: Iterable[tuple[int, int]] = ()) -> list[re.Match[str]]:
        candidates = list(self.name_pattern.finditer(text))
        if not candidates:
            return []
        starts, ends = merge_spans([*self.get_mask_spans(text), *extra_spans])
        return [match for match in candidates if not is_protected(match.start(), match.end(), starts, ends)]

    def find_path_matches(self, path: str) -> list[re.Match[str]]:
        extra: list[tuple[int, int]] = []
        kept = self.get_kept_component_indexes(path)
        if kept:
            offset = 0
            for index, component in enumerate(path.split("/")):
                if index in kept:
                    extra.append((offset, offset + len(component)))
                offset += len(component) + 1
        return self.find_unmasked(path, extra)

    def find_text_matches(self, text: str, *paths: str) -> list[re.Match[str]]:
        rules = self.allowlist.get_rules(*paths)
        if any(rule.scope is None for rule in rules):
            return []
        extra = [span for rule in rules for span in get_scope_spans(rule, text)]
        return self.find_unmasked(text, extra)


def is_git_work_tree(root: pathlib.Path) -> bool:
    try:
        result = subprocess.run(
            ["git", "-C", str(root), "rev-parse", "--is-inside-work-tree"],
            capture_output=True,
            text=True,
        )
    except FileNotFoundError:
        return False
    return result.returncode == 0 and result.stdout.strip() == "true"


def list_files(root: pathlib.Path) -> list[str]:
    if not root.is_dir():
        raise FileListingError(f"{root} is not a directory")
    if is_git_work_tree(root):
        result = subprocess.run(
            ["git", "-C", str(root), "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
            capture_output=True,
        )
        if result.returncode != 0:
            raise FileListingError(
                f"git ls-files failed in {root}: {result.stderr.decode('utf-8', 'replace').strip()}"
            )
        listed = {entry for entry in result.stdout.decode("utf-8", "surrogateescape").split("\0") if entry}
    else:
        listed = set()
        for directory, subdirectories, filenames in os.walk(root):
            subdirectories[:] = sorted(name for name in subdirectories if name not in WALK_SKIP_DIRS)
            relative = pathlib.Path(directory).relative_to(root)
            listed.update((relative / name).as_posix() for name in filenames)
    return sorted(
        path for path in listed if (root / path).is_file() and not (root / path).is_symlink()
    )


def read_text_file(path: pathlib.Path) -> str | None:
    data = path.read_bytes()
    if b"\0" in data[:BINARY_SNIFF_BYTES]:
        return None
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return None


def get_hit_token(text: str, start: int, end: int) -> str:
    left = start
    while left > 0 and start - left < TOKEN_CONTEXT_LIMIT and TOKEN_CHAR.match(text[left - 1]):
        left -= 1
    right = end
    while right < len(text) and right - end < TOKEN_CONTEXT_LIMIT and TOKEN_CHAR.match(text[right]):
        right += 1
    token = text[left:right]
    if text.startswith("://", right):
        token += "://"
    return token


def get_line_starts(text: str) -> list[int]:
    return [0, *(match.end() for match in re.finditer("\n", text))]


def find_hits(root: pathlib.Path, scanner: NameScanner, files: Sequence[str]) -> list[Hit]:
    hits: list[Hit] = []
    for path in files:
        for match in scanner.find_path_matches(path):
            hits.append(Hit(path, 0, match.start() + 1, get_hit_token(path, match.start(), match.end())))
        text = read_text_file(root / path)
        if text is None:
            continue
        matches = scanner.find_text_matches(text, path)
        if not matches:
            continue
        line_starts = get_line_starts(text)
        for match in matches:
            line = bisect.bisect_right(line_starts, match.start())
            column = match.start() - line_starts[line - 1] + 1
            hits.append(Hit(path, line, column, get_hit_token(text, match.start(), match.end())))
    return hits


def build_scanner(root: pathlib.Path, old_name: str, stage: str, allowlist_path: pathlib.Path | None) -> tuple[NameScanner, list[str]]:
    allowlist = load_allowlist(allowlist_path or root / ALLOWLIST_PATH)
    files = list_files(root)
    return NameScanner(get_name_forms(old_name), stage, allowlist, files), files


def parse_args(argv: Sequence[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--stage", required=True, choices=STAGES)
    parser.add_argument("--old", default=DEFAULT_OLD_NAME, help="the name being retired")
    parser.add_argument("--root", type=pathlib.Path, default=pathlib.Path(__file__).resolve().parent.parent)
    parser.add_argument("--allowlist", type=pathlib.Path, default=None)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    root = args.root.resolve()
    try:
        scanner, files = build_scanner(root, args.old, args.stage, args.allowlist)
        hits = find_hits(root, scanner, files)
    except OldNameError as error:
        print(f"check_old_name: {error}", file=sys.stderr)
        return 2
    for hit in hits:
        print(hit.format())
    print(f"{len(hits)} {'hit' if len(hits) == 1 else 'hits'} of {args.old} outside the allowlist")
    return 1 if hits else 0


if __name__ == "__main__":
    sys.exit(main())
