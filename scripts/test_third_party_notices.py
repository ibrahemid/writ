import importlib.util
import json
import pathlib
import tempfile
import unittest


SCRIPT_PATH = pathlib.Path(__file__).with_name("gen-third-party-notices.py")
SPEC = importlib.util.spec_from_file_location("gen_third_party_notices", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Could not load {SCRIPT_PATH}")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

MIT_BODY = "Permission is hereby granted, free of charge, to any person obtaining a copy"
QUALIFIED_MIT = f"MIT License\n\nCopyright (c) 2017 Someone\n\n{MIT_BODY}\n"
UNQUALIFIED_MIT = f"MIT License\n\nCopyright (c) 2017 Someone\n\n\n{MIT_BODY}\n"
TEMPLATE = f"MIT License\n\nCopyright (c) <year> <copyright holders>\n\n{MIT_BODY}\n"


class ResolveCrateTextTests(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.crate_dir = pathlib.Path(tmp.name)
        (self.crate_dir / "Cargo.toml").write_text("[package]\n")
        self.krate = {
            "name": "twice-licensed",
            "version": "0.1.0",
            "manifest_path": str(self.crate_dir / "Cargo.toml"),
        }

    def resolve(self, about_text: str) -> tuple[str, str]:
        return MODULE.resolve_crate_text("MIT", about_text, self.krate, {}, set())

    def test_the_file_named_for_the_licence_wins_whichever_file_cargo_about_read(self) -> None:
        (self.crate_dir / "LICENSE").write_text(UNQUALIFIED_MIT)
        (self.crate_dir / "LICENSE-MIT.md").write_text(QUALIFIED_MIT)

        for about_text in (UNQUALIFIED_MIT, QUALIFIED_MIT):
            self.assertEqual(
                self.resolve(MODULE.normalize_text(about_text)),
                (MODULE.normalize_text(QUALIFIED_MIT), "shipped"),
            )

    def test_cargo_about_text_stands_when_the_crate_ships_no_file_named_for_the_licence(self) -> None:
        (self.crate_dir / "LICENSE").write_text(UNQUALIFIED_MIT)

        about_text = MODULE.normalize_text(QUALIFIED_MIT)
        self.assertEqual(self.resolve(about_text), (about_text, "about"))

    def test_an_unqualified_licence_file_replaces_only_a_template(self) -> None:
        (self.crate_dir / "LICENSE").write_text(UNQUALIFIED_MIT)

        self.assertEqual(
            self.resolve(MODULE.normalize_text(TEMPLATE)),
            (MODULE.normalize_text(UNQUALIFIED_MIT), "shipped"),
        )

    def test_a_crate_with_no_file_and_no_record_is_refused(self) -> None:
        with self.assertRaises(MODULE.GenerationError):
            self.resolve(MODULE.normalize_text(TEMPLATE))



def write_package(directory: pathlib.Path, manifest: dict[str, object], licence_text: str | None = None) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "package.json").write_text(json.dumps(manifest))
    if licence_text is not None:
        (directory / "LICENSE").write_text(licence_text)


class MermaidClosureTests(unittest.TestCase):
    """A pnpm layout: each package under .pnpm/<id>/node_modules/<name>, its
    dependencies linked beside it."""

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        store = self.root / "node_modules" / ".pnpm"
        self.mermaid = store / "mermaid@1.0.0" / "node_modules" / "mermaid"
        write_package(
            self.mermaid,
            {
                "name": "mermaid",
                "version": "1.0.0",
                "license": "MIT",
                "dependencies": {"dompurify": "^3", "@types/d3": "^7", "khroma": "^2"},
                "optionalDependencies": {"absent": "^1"},
            },
            QUALIFIED_MIT,
        )
        purify = store / "dompurify@3.4.0" / "node_modules" / "dompurify"
        write_package(
            purify,
            {"name": "dompurify", "version": "3.4.0", "license": "(MPL-2.0 OR Apache-2.0)", "dependencies": {"khroma": "^2"}},
            "Apache License\nVersion 2.0\n",
        )
        khroma = store / "khroma@2.1.0" / "node_modules" / "khroma"
        write_package(khroma, {"name": "khroma", "version": "2.1.0"}, f"The MIT License (MIT)\n\n{MIT_BODY}\n")
        for owner in (self.mermaid, purify):
            for name, target in (("dompurify", purify), ("khroma", khroma)):
                link = owner.parent / name
                if not link.exists():
                    link.symlink_to(target)

    def test_follows_runtime_dependencies_through_the_pnpm_store(self) -> None:
        closure = MODULE.runtime_closure(self.mermaid)
        self.assertEqual(
            [(name, version, licence) for name, version, _, licence in closure],
            [("dompurify", "3.4.0", "(MPL-2.0 OR Apache-2.0)"), ("khroma", "2.1.0", "UNKNOWN")],
        )

    def test_a_missing_required_dependency_is_an_error(self) -> None:
        manifest = json.loads((self.mermaid / "package.json").read_text())
        manifest["dependencies"]["gone"] = "^1"
        (self.mermaid / "package.json").write_text(json.dumps(manifest))
        with self.assertRaisesRegex(MODULE.GenerationError, "needs gone"):
            MODULE.runtime_closure(self.mermaid)

    def test_groups_take_the_accepted_choice_and_name_an_undeclared_mit_file(self) -> None:
        groups, declarations, without_text = MODULE.group_package_licences(
            MODULE.runtime_closure(self.mermaid), ["MIT", "Apache-2.0", "MPL-2.0"]
        )
        self.assertEqual(
            [(group["id"], group["packages"]) for group in groups],
            [("Apache-2.0", [("dompurify", "3.4.0")]), ("MIT", [("khroma", "2.1.0")])],
        )
        self.assertEqual((declarations, without_text), ([], []))


class RenderTests(unittest.TestCase):
    def render(self, **mermaid: object) -> str:
        mpl = {"id": "MPL-2.0", "name": "Mozilla Public License 2.0", "text": "MPL text", "crates": [("cssparser", "0.37.0")], "attribution": {}}
        return MODULE.render([mpl], [], 1, [], [], 0, [], [], **mermaid)

    def test_every_mpl_crate_names_where_its_source_is(self) -> None:
        self.assertIn("- cssparser 0.37.0: https://crates.io/crates/cssparser/0.37.0", self.render())

    def test_the_mermaid_bundle_gets_its_own_section(self) -> None:
        group = {"id": "Apache-2.0", "text": "Apache text", "packages": [("dompurify", "3.4.0")]}
        text = self.render(mermaid_groups=[group], mermaid_package_count=1)
        self.assertIn("## Packages inside the Mermaid bundle", text)
        self.assertIn("1 more npm packages ship inside the Mermaid bundle", text)
        self.assertIn("dompurify 3.4.0", text.split("## Packages inside the Mermaid bundle")[1])
        self.assertIn("| Apache-2.0 |  | 1 |  |", text)


if __name__ == "__main__":
    unittest.main()
