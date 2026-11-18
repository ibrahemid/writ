#!/usr/bin/env python3
"""Rename the product across the tree: paths, then text, then generated outputs, then the guard."""

from __future__ import annotations

import argparse
import dataclasses
import pathlib
import re
import shutil
import subprocess
import sys
from typing import Sequence

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import check_old_name as names  # noqa: E402

GENERATED_GLOBS = (
    "Cargo.lock",
    "src/styles/generated/**",
    "src-tauri/assets/generated/**",
    "site/src/styles/tokens.css",
    "THIRD-PARTY-NOTICES.md",
    "src-tauri/gen/schemas/**",
    "src-tauri/wix/legacy-progids.wxs",
    "site/src/data/release.json",
)
GENERATED_PATTERNS = tuple(names.compile_glob(glob) for glob in GENERATED_GLOBS)
STDERR_TAIL_LINES = 8


class PathConflictError(names.OldNameError):
    """Two files would end up at one path, or a renamed path is already taken."""


@dataclasses.dataclass(frozen=True)
class Generator:
    outputs: str
    source: str
    command: tuple[str, ...] = ()
    required_tools: tuple[str, ...] = ()
    required_paths: tuple[str, ...] = ()


GENERATORS = (
    Generator("Cargo.lock", "cargo", ("cargo", "update", "--workspace"), ("cargo",), ("Cargo.toml",)),
    Generator("Rust import order", "cargo fmt", ("cargo", "fmt", "--all"), ("cargo",), ("Cargo.toml",)),
    Generator(
        "CSS tokens",
        "pnpm tokens:build",
        ("pnpm", "tokens:build"),
        ("pnpm",),
        ("node_modules", "design/tokens/config.mjs"),
    ),
    Generator(
        "THIRD-PARTY-NOTICES.md",
        "scripts/gen-third-party-notices.py",
        (sys.executable, "scripts/gen-third-party-notices.py"),
        ("cargo", "cargo-about"),
        ("node_modules", "scripts/gen-third-party-notices.py"),
    ),
    Generator("src-tauri/gen/schemas", "tauri-build on cargo build"),
    Generator("src-tauri/wix/legacy-progids.wxs", "scripts/gen-legacy-progids.py"),
    Generator("site/src/data/release.json", "the site.yml sync"),
    Generator("stills, loops and README GIFs", "scripts/capture/run.sh"),
    Generator("site/public/brand/og-1-0.png", "its source drawing"),
)


@dataclasses.dataclass(frozen=True)
class PathMove:
    old: str
    new: str


@dataclasses.dataclass(frozen=True)
class ContentEdit:
    path: str
    text: str
    replacements: int


@dataclasses.dataclass(frozen=True)
class RenamePlan:
    moves: tuple[PathMove, ...]
    edits: tuple[ContentEdit, ...]

    @property
    def replacement_count(self) -> int:
        return sum(edit.replacements for edit in self.edits)

    @property
    def is_empty(self) -> bool:
        return not self.moves and not self.edits


def validate_new_name(old: names.NameForms, new: names.NameForms) -> None:
    if new.display == old.display:
        raise names.InvalidNameError(f"the new name equals the old name {old.display!r}")
    old_pattern = names.compile_name_pattern(old)
    for form in (new.display, new.lower, new.upper):
        for probe in (form, f"a{form}", f"0{form}"):
            if old_pattern.search(probe) is not None:
                raise names.InvalidNameError(
                    f"{new.display!r} spells {old.display!r} where the rename matches it, "
                    "so a second run would rename it again"
                )


def is_generated(path: str) -> bool:
    return any(pattern.fullmatch(path) is not None for pattern in GENERATED_PATTERNS)


def rename_matches(text: str, matches: Sequence[re.Match[str]], old: names.NameForms, new: names.NameForms) -> str:
    pieces: list[str] = []
    last = 0
    for match in matches:
        pieces.append(text[last : match.start()])
        pieces.append(new.get_replacement(match.group(), old))
        last = match.end()
    pieces.append(text[last:])
    return "".join(pieces)


def select_stage_matches(
    scanner: names.NameScanner, text: str, matches: Sequence[re.Match[str]]
) -> list[re.Match[str]]:
    if scanner.stage != names.CHANNELS_STAGE or not matches:
        return list(matches)
    starts, ends = names.merge_spans(scanner.get_held_back_spans(text))
    return [match for match in matches if names.is_protected(match.start(), match.end(), starts, ends)]


def get_new_path(scanner: names.NameScanner, new: names.NameForms, path: str) -> str:
    matches = select_stage_matches(scanner, path, scanner.find_path_matches(path))
    return rename_matches(path, matches, scanner.old, new) if matches else path


def validate_moves(root: pathlib.Path, files: Sequence[str], moves: Sequence[PathMove]) -> None:
    moving = {move.old: move.new for move in moves}
    claimed: dict[str, str] = {}
    for path in files:
        target = moving.get(path, path)
        if target in claimed:
            raise PathConflictError(f"{claimed[target]} and {path} would both become {target}")
        claimed[target] = path
    for move in moves:
        if move.new not in moving and (root / move.new).exists():
            raise PathConflictError(f"{move.old} would become {move.new}, which already exists")


def plan_rename(
    root: pathlib.Path, files: Sequence[str], scanner: names.NameScanner, new: names.NameForms
) -> RenamePlan:
    targets = {path: get_new_path(scanner, new, path) for path in files}
    moves = tuple(PathMove(path, target) for path, target in targets.items() if target != path)
    validate_moves(root, files, moves)
    edits: list[ContentEdit] = []
    for path, target in targets.items():
        if is_generated(path) or is_generated(target) or scanner.allowlist.is_whole_file(path, target):
            continue
        text = names.read_text_file(root / path)
        if text is None:
            continue
        matches = select_stage_matches(scanner, text, scanner.find_text_matches(text, path, target))
        if matches:
            edits.append(ContentEdit(target, rename_matches(text, matches, scanner.old, new), len(matches)))
    return RenamePlan(moves, tuple(edits))


def get_directory_moves(files: Sequence[str], moves: Sequence[PathMove]) -> list[tuple[str, str]]:
    targets = {move.old: move.new for move in moves}
    candidates: dict[str, str] = {}
    rejected: set[str] = set()
    for move in moves:
        old_parts = move.old.split("/")
        new_parts = move.new.split("/")
        for depth in range(1, len(old_parts)):
            if old_parts[depth - 1] == new_parts[depth - 1]:
                continue
            old_dir = "/".join(old_parts[:depth])
            new_dir = "/".join(new_parts[:depth])
            if candidates.setdefault(old_dir, new_dir) != new_dir:
                rejected.add(old_dir)
    for old_dir, new_dir in candidates.items():
        prefix = old_dir + "/"
        for path in files:
            if path.startswith(prefix) and not targets.get(path, path).startswith(new_dir + "/"):
                rejected.add(old_dir)
                break
    accepted = [(old_dir, new_dir) for old_dir, new_dir in candidates.items() if old_dir not in rejected]
    return sorted(accepted, key=lambda item: (item[0].count("/"), item[0]))


def locate_path(path: str, done: Sequence[tuple[str, str]]) -> str:
    for old_dir, new_dir in reversed(done):
        if path == old_dir or path.startswith(old_dir + "/"):
            return new_dir + path[len(old_dir) :]
    return path


def remove_empty_directories(root: pathlib.Path, moves: Sequence[PathMove]) -> None:
    directories: set[str] = set()
    for move in moves:
        parts = move.old.split("/")
        directories.update("/".join(parts[:depth]) for depth in range(1, len(parts)))
    for directory in sorted(directories, key=lambda item: item.count("/"), reverse=True):
        path = root / directory
        if path.is_dir() and not path.is_symlink() and not any(path.iterdir()):
            path.rmdir()


def apply_moves(root: pathlib.Path, files: Sequence[str], moves: Sequence[PathMove]) -> None:
    done: list[tuple[str, str]] = []
    for old_dir, new_dir in get_directory_moves(files, moves):
        source = root / locate_path(old_dir, done)
        target = root / new_dir
        if not source.is_dir() or target.exists():
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        source.rename(target)
        done.append((old_dir, new_dir))
    for move in moves:
        current = locate_path(move.old, done)
        if current == move.new:
            continue
        target = root / move.new
        if target.exists():
            raise PathConflictError(f"{move.old} would become {move.new}, which already exists")
        target.parent.mkdir(parents=True, exist_ok=True)
        (root / current).rename(target)
    remove_empty_directories(root, moves)


def apply_plan(root: pathlib.Path, files: Sequence[str], plan: RenamePlan) -> None:
    apply_moves(root, files, plan.moves)
    for edit in plan.edits:
        (root / edit.path).write_bytes(edit.text.encode("utf-8"))


def get_missing_requirements(root: pathlib.Path, generator: Generator) -> list[str]:
    missing = [tool for tool in generator.required_tools if shutil.which(tool) is None]
    missing.extend(path for path in generator.required_paths if not (root / path).exists())
    return missing


def run_generators(root: pathlib.Path, generators: Sequence[Generator] = GENERATORS) -> list[str]:
    failures: list[str] = []
    for generator in generators:
        if not generator.command:
            print(f"pending: {generator.outputs} <- {generator.source}")
            continue
        missing = get_missing_requirements(root, generator)
        if missing:
            print(f"skipped: {generator.outputs} <- {generator.source}; needs {', '.join(missing)}")
            continue
        result = subprocess.run(generator.command, cwd=root, capture_output=True, text=True)
        if result.returncode == 0:
            print(f"generated: {generator.outputs} <- {generator.source}")
            continue
        print(f"failed: {generator.outputs} <- {generator.source}; exit {result.returncode}")
        for line in result.stderr.strip().splitlines()[-STDERR_TAIL_LINES:]:
            print(f"  {line}")
        failures.append(generator.outputs)
    return failures


def print_plan(plan: RenamePlan) -> None:
    for move in plan.moves:
        print(f"rename {move.old} -> {move.new}")
    for edit in plan.edits:
        print(f"edit {edit.path} ({edit.replacements})")
    print(f"{len(plan.moves)} paths to rename, {len(plan.edits)} files to edit, {plan.replacement_count} replacements")


def parse_args(argv: Sequence[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--name", required=True, help="the new product name, such as Example")
    parser.add_argument("--from", dest="old_name", default=names.DEFAULT_OLD_NAME, help="the name being retired")
    parser.add_argument("--stage", required=True, choices=names.STAGES)
    parser.add_argument("--check", action="store_true", help="write nothing; exit 1 when anything would change")
    parser.add_argument("--root", type=pathlib.Path, default=pathlib.Path(__file__).resolve().parent.parent)
    parser.add_argument("--allowlist", type=pathlib.Path, default=None)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    root = args.root.resolve()
    try:
        old = names.get_name_forms(args.old_name)
        new = names.get_name_forms(args.name)
        validate_new_name(old, new)
        allowlist = names.load_allowlist(args.allowlist or root / names.ALLOWLIST_PATH)
        files = names.list_files(root)
        scanner = names.NameScanner(old, args.stage, allowlist, files)
        plan = plan_rename(root, files, scanner, new)
        if args.check:
            print_plan(plan)
            return 0 if plan.is_empty else 1
        apply_plan(root, files, plan)
        print(f"renamed {len(plan.moves)} paths, edited {len(plan.edits)} files, {plan.replacement_count} replacements")
        failures = run_generators(root)
        renamed_files = names.list_files(root)
        guard = names.NameScanner(old, args.stage, allowlist, renamed_files)
        hits = names.find_hits(root, guard, renamed_files)
    except (names.OldNameError, OSError) as error:
        print(f"rename_product: {error}", file=sys.stderr)
        return 2
    for hit in hits:
        print(hit.format())
    print(f"{len(hits)} {'hit' if len(hits) == 1 else 'hits'} of {old.display} outside the allowlist")
    return 1 if hits or failures else 0


if __name__ == "__main__":
    sys.exit(main())
