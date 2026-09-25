import importlib.util
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


if __name__ == "__main__":
    unittest.main()
