"""Tests for the WiX fragments that carry installs made under the old product name.

Run from the repository root:
    python3 -m unittest discover -s scripts -p 'test_*.py'
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import pathlib
import re
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
from unittest import mock

SCRIPT = pathlib.Path(__file__).with_name("gen-legacy-progids.py")
SPEC = importlib.util.spec_from_file_location("gen_legacy_progids", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Could not load {SCRIPT}")
GENERATOR = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = GENERATOR
SPEC.loader.exec_module(GENERATOR)

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC_TAURI = ROOT / "src-tauri"
TAURI_CONF = SRC_TAURI / "tauri.conf.json"
WIX_NS = "http://schemas.microsoft.com/wix/2006/wi"
NS = {"w": WIX_NS}

UPGRADE_CODE = "ab97cc4d-eea3-53ee-90d8-3c2767967676"
LEGACY_NAME = "Writ"
LEGACY_LAUNCHER = "writ-tauri.exe"
OLD_PATH_COMPONENT_ID = "WritCommandOnPath"
OLD_PATH_COMPONENT_GUID = "E48693D8-AA17-4D27-9EAB-F2A34A39862D"
FRAGMENT_PATHS = ["wix/cli-path.wxs", "wix/legacy-install.wxs", "wix/legacy-progids.wxs"]
COMPONENT_GUIDS = {
    "JotraCommandOnPath": "CED06BBB-8BF4-46A7-9C2F-1B72C3806767",
    "LegacyShortcuts": "BADF7A83-7D05-4999-A072-D039570F8E37",
    "LegacyLauncher": "7E25E986-7C0A-439A-8477-CADA64838A09",
    "LegacyProgIds": "E808E13F-A233-4FE4-929B-FB5AA08886C0",
}
TEMPLATE_DIRECTORIES = {
    "TARGETDIR",
    "DesktopFolder",
    "ProgramFiles64Folder",
    "INSTALLDIR",
    "ProgramMenuFolder",
    "ApplicationProgramsFolder",
}
TEMPLATE_FILE_IDS = {"Path"}
GUID_PATTERN = re.compile(r"[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}")
OPEN_COMMAND = '"[#Path]" "%1"'


def tag(name: str) -> str:
    return f"{{{WIX_NS}}}{name}"


def load_conf() -> dict:
    return json.loads(TAURI_CONF.read_text(encoding="utf-8"))


def load_wix_conf() -> dict:
    return load_conf()["bundle"]["windows"]["wix"]


def load_fragments() -> dict[str, ET.Element]:
    return {path: ET.parse(SRC_TAURI / path).getroot() for path in load_wix_conf()["fragmentPaths"]}


def load_fragment(path: str) -> ET.Element:
    return ET.parse(SRC_TAURI / path).getroot()


def components_of(root: ET.Element) -> list[ET.Element]:
    return list(root.iter(tag("Component")))


def find_one(root: ET.Element, name: str, **attrs: str) -> ET.Element:
    found = [
        element
        for element in root.iter(tag(name))
        if all(element.get(key) == value for key, value in attrs.items())
    ]
    if len(found) != 1:
        raise AssertionError(f"expected one {name} with {attrs}, found {len(found)}")
    return found[0]


def default_value_of(key: ET.Element) -> str | None:
    values = [value for value in key.findall("w:RegistryValue", NS) if value.get("Name") is None]
    if len(values) != 1:
        return None
    return values[0].get("Value")


def legacy_extensions() -> list[str]:
    return [
        ext[1:] if ext.startswith(".") else ext
        for association in load_conf()["bundle"]["fileAssociations"]
        for ext in association["ext"]
    ]


class WixConfigTests(unittest.TestCase):
    def test_the_upgrade_code_is_pinned(self) -> None:
        self.assertEqual(load_wix_conf()["upgradeCode"], UPGRADE_CODE)

    def test_the_fragment_paths_are_the_wix_folder(self) -> None:
        paths = load_wix_conf()["fragmentPaths"]
        self.assertEqual(paths, FRAGMENT_PATHS)
        on_disk = sorted(f"wix/{path.name}" for path in (SRC_TAURI / "wix").glob("*.wxs"))
        self.assertEqual(sorted(paths), on_disk)

    def test_component_refs_name_every_fragment_component(self) -> None:
        wix = load_wix_conf()
        defined = [component.get("Id") for root in load_fragments().values() for component in components_of(root)]
        self.assertEqual(len(defined), len(set(defined)))
        self.assertEqual(sorted(wix["componentRefs"]), sorted(defined))
        for key in ("componentGroupRefs", "featureGroupRefs", "featureRefs", "mergeRefs"):
            with self.subTest(key=key):
                self.assertFalse(wix.get(key))

    def test_each_fragment_is_linked_by_a_component_ref(self) -> None:
        refs = set(load_wix_conf()["componentRefs"])
        for path, root in load_fragments().items():
            with self.subTest(path=path):
                fragments = root.findall("w:Fragment", NS)
                self.assertEqual(len(fragments), 1)
                self.assertTrue(refs & {component.get("Id") for component in components_of(root)})


class WixFragmentTests(unittest.TestCase):
    def test_every_fragment_parses_as_wix(self) -> None:
        for path, root in load_fragments().items():
            with self.subTest(path=path):
                self.assertEqual(root.tag, tag("Wix"))

    def test_no_fragment_holds_template_braces(self) -> None:
        for path in FRAGMENT_PATHS:
            with self.subTest(path=path):
                self.assertNotIn("{{", (SRC_TAURI / path).read_text(encoding="utf-8"))

    def test_component_guids_match_the_table(self) -> None:
        found = {
            component.get("Id"): component.get("Guid")
            for root in load_fragments().values()
            for component in components_of(root)
        }
        self.assertEqual(found, COMPONENT_GUIDS)
        self.assertEqual(len(set(found.values())), len(found))
        for component_id, guid in found.items():
            with self.subTest(component=component_id):
                self.assertRegex(guid, GUID_PATTERN)
                self.assertNotEqual(guid.upper(), OLD_PATH_COMPONENT_GUID)
        self.assertEqual(GENERATOR.COMPONENT_GUID, COMPONENT_GUIDS[GENERATOR.COMPONENT_ID])

    def test_the_old_path_component_is_gone(self) -> None:
        for path in FRAGMENT_PATHS:
            text = (SRC_TAURI / path).read_text(encoding="utf-8")
            with self.subTest(path=path):
                self.assertNotIn(OLD_PATH_COMPONENT_GUID.lower(), text.lower())
                self.assertNotIn(OLD_PATH_COMPONENT_ID, text)
                self.assertNotIn(f"Software\\{LEGACY_NAME}\\Command", text)

    def test_the_path_component_has_the_new_key_path(self) -> None:
        component = find_one(load_fragment("wix/cli-path.wxs"), "Component", Id="JotraCommandOnPath")
        self.assertEqual(component.get("Guid"), COMPONENT_GUIDS["JotraCommandOnPath"])
        key_path = find_one(component, "RegistryValue", KeyPath="yes")
        self.assertEqual(key_path.get("Root"), "HKCU")
        self.assertEqual(key_path.get("Key"), "Software\\Jotra\\Command")
        environment = find_one(component, "Environment", Name="PATH")
        self.assertEqual(environment.get("Value"), "[INSTALLDIR]")
        self.assertEqual(environment.get("Part"), "last")
        self.assertEqual(environment.get("System"), "yes")

    def test_directory_references_resolve(self) -> None:
        roots = load_fragments().values()
        defined = {directory.get("Id") for root in roots for directory in root.iter(tag("Directory"))}
        self.assertFalse(defined & TEMPLATE_DIRECTORIES)
        known = TEMPLATE_DIRECTORIES | defined
        for root in roots:
            for element in root.iter(tag("DirectoryRef")):
                self.assertIn(element.get("Id"), TEMPLATE_DIRECTORIES)
            for name in ("RemoveFile", "RemoveFolder"):
                for element in root.iter(tag(name)):
                    with self.subTest(element=element.get("Id")):
                        self.assertIn(element.get("Directory"), known)

    def test_formatted_strings_reference_template_files_by_long_path(self) -> None:
        for path, root in load_fragments().items():
            for element in root.iter():
                for value in element.attrib.values():
                    with self.subTest(path=path, value=value):
                        self.assertNotRegex(value, r"\[[!$]")
                        for file_id in re.findall(r"\[#([^\]]+)\]", value):
                            self.assertIn(file_id, TEMPLATE_FILE_IDS)


class LegacyInstallFolderTests(unittest.TestCase):
    def setUp(self) -> None:
        self.root = load_fragment("wix/legacy-install.wxs")

    def test_the_registry_search_reads_the_old_install_dir(self) -> None:
        publisher = load_conf()["bundle"]["publisher"]
        prop = find_one(self.root, "Property", Id="LEGACYINSTALLDIR")
        self.assertEqual(prop.get("Secure"), "yes")
        search = find_one(prop, "RegistrySearch")
        self.assertEqual(search.get("Root"), "HKCU")
        self.assertEqual(search.get("Key"), f"Software\\{publisher}\\{LEGACY_NAME}")
        self.assertEqual(search.get("Name"), "InstallDir")
        self.assertEqual(search.get("Type"), "directory")

    def test_the_file_search_finds_the_old_binary_in_the_default_folder(self) -> None:
        prop = find_one(self.root, "Property", Id="LEGACYLAUNCHER")
        self.assertEqual(prop.get("Secure"), "yes")
        directory = find_one(prop, "DirectorySearch")
        self.assertEqual(directory.get("Path"), f"[ProgramFiles64Folder]{LEGACY_NAME}")
        self.assertEqual(directory.get("Depth"), "0")
        file_search = find_one(directory, "FileSearch")
        self.assertEqual(file_search.get("Name"), LEGACY_LAUNCHER)
        self.assertIsNone(file_search.get("ShortName"))

    def test_install_dir_is_set_from_the_registry_then_the_file_before_costing(self) -> None:
        actions = {element.get("Action"): element for element in self.root.iter(tag("SetProperty"))}
        self.assertEqual(set(actions), {"SetInstallDirFromLegacyKey", "SetInstallDirFromLegacyLauncher"})
        for action in actions.values():
            with self.subTest(action=action.get("Action")):
                self.assertEqual(action.get("Id"), "INSTALLDIR")
                self.assertEqual(action.get("Sequence"), "both")
                self.assertIsNone(action.get("After"))

        order = []
        current = "SetInstallDirFromLegacyKey"
        while current in actions:
            order.append(current)
            current = actions[current].get("Before")
        self.assertEqual(order, ["SetInstallDirFromLegacyKey", "SetInstallDirFromLegacyLauncher"])
        self.assertEqual(current, "CostFinalize")

        registry = actions["SetInstallDirFromLegacyKey"]
        self.assertEqual(registry.get("Value"), "[LEGACYINSTALLDIR]")
        self.assertEqual(registry.text, "NOT Installed AND NOT INSTALLDIR AND LEGACYINSTALLDIR")
        launcher = actions["SetInstallDirFromLegacyLauncher"]
        self.assertEqual(launcher.get("Value"), f"[ProgramFiles64Folder]{LEGACY_NAME}\\")
        self.assertEqual(launcher.text, "NOT Installed AND NOT INSTALLDIR AND LEGACYLAUNCHER")

    def test_conditions_name_only_defined_search_properties(self) -> None:
        defined = {prop.get("Id") for prop in self.root.iter(tag("Property"))}
        for action in self.root.iter(tag("SetProperty")):
            used = set(re.findall(r"\bLEGACY[A-Z]+\b", f"{action.text} {action.get('Value')}"))
            with self.subTest(action=action.get("Action")):
                self.assertTrue(used)
                self.assertLessEqual(used, defined)

    def test_old_shortcuts_are_removed_at_install(self) -> None:
        component = find_one(self.root, "Component", Id="LegacyShortcuts")
        removed = {
            (element.get("Directory"), element.get("Name"), element.get("On"))
            for element in component.findall("w:RemoveFile", NS)
        }
        self.assertEqual(
            removed,
            {
                ("LegacyProgramsFolder", f"{LEGACY_NAME}.lnk", "install"),
                ("DesktopFolder", f"{LEGACY_NAME}.lnk", "install"),
                ("INSTALLDIR", f"Uninstall {LEGACY_NAME}.lnk", "install"),
            },
        )
        folder = find_one(component, "RemoveFolder")
        self.assertEqual((folder.get("Directory"), folder.get("On")), ("LegacyProgramsFolder", "install"))
        self.assertIsNotNone(component.find("w:CreateFolder", NS))

    def test_the_old_start_menu_folder_sits_under_the_program_menu(self) -> None:
        parent = find_one(self.root, "DirectoryRef", Id="ProgramMenuFolder")
        directory = find_one(parent, "Directory", Id="LegacyProgramsFolder")
        self.assertEqual(directory.get("Name"), LEGACY_NAME)

    def test_the_launcher_installs_beside_the_gui(self) -> None:
        component = find_one(self.root, "Component", Id="LegacyLauncher")
        file_element = find_one(component, "File")
        self.assertEqual(file_element.get("Name"), LEGACY_LAUNCHER)
        self.assertEqual(file_element.get("Source"), f"$(sys.SOURCEFILEDIR)..\\binaries\\{LEGACY_LAUNCHER}")
        self.assertEqual(file_element.get("KeyPath"), "yes")
        self.assertEqual(find_one(self.root, "DirectoryRef", Id="INSTALLDIR"), self.find_parent_ref(component))

    def find_parent_ref(self, component: ET.Element) -> ET.Element | None:
        for ref in self.root.iter(tag("DirectoryRef")):
            if component in list(ref):
                return ref
        return None


class LegacyProgIdTests(unittest.TestCase):
    def setUp(self) -> None:
        self.root = load_fragment("wix/legacy-progids.wxs")
        self.component = find_one(self.root, "Component", Id="LegacyProgIds")

    def test_every_association_has_its_legacy_progid(self) -> None:
        expected = [f"Software\\Classes\\{LEGACY_NAME}.{ext}" for ext in legacy_extensions()]
        keys = self.component.findall("w:RegistryKey", NS)
        self.assertEqual([key.get("Key") for key in keys], expected)
        self.assertTrue(expected)

    def test_each_progid_opens_the_new_binary(self) -> None:
        verb = f"Open with {load_conf()['productName']}"
        for key in self.component.findall("w:RegistryKey", NS):
            with self.subTest(key=key.get("Key")):
                self.assertEqual(key.get("Root"), "HKLM")
                shell_open = find_one(key, "RegistryKey", Key="shell\\open")
                self.assertEqual(default_value_of(shell_open), verb)
                command = find_one(shell_open, "RegistryKey", Key="command")
                self.assertEqual(default_value_of(command), OPEN_COMMAND)

    def test_the_component_has_one_key_path(self) -> None:
        key_paths = [value for value in self.component.iter(tag("RegistryValue")) if value.get("KeyPath") == "yes"]
        self.assertEqual(len(key_paths), 1)
        self.assertEqual(key_paths[0].get("Value"), OPEN_COMMAND)

    def test_the_committed_fragment_matches_the_generator(self) -> None:
        self.assertEqual(
            GENERATOR.generate(TAURI_CONF),
            (SRC_TAURI / "wix" / "legacy-progids.wxs").read_text(encoding="utf-8"),
        )


class GeneratorTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.output = pathlib.Path(self.tmp.name) / "legacy-progids.wxs"

    def run_main(self, *argv: str) -> int:
        with mock.patch.object(GENERATOR, "OUTPUT_PATH", self.output):
            with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                return GENERATOR.main(list(argv))

    def conf(self, associations: list[dict], product_name: str = "Jotra") -> dict:
        return {"productName": product_name, "bundle": {"fileAssociations": associations}}

    def test_write_then_check_passes(self) -> None:
        self.assertEqual(self.run_main(), 0)
        self.assertEqual(self.output.read_text(encoding="utf-8"), GENERATOR.generate(TAURI_CONF))
        self.assertEqual(self.run_main("--check"), 0)

    def test_check_fails_on_a_stale_fragment(self) -> None:
        self.output.write_text(GENERATOR.generate(TAURI_CONF).replace(".txt", ".text", 1), encoding="utf-8")
        self.assertEqual(self.run_main("--check"), 1)

    def test_check_fails_when_the_fragment_is_missing(self) -> None:
        self.assertEqual(self.run_main("--check"), 1)

    def test_a_leading_dot_is_stripped(self) -> None:
        associations = GENERATOR.get_associations(self.conf([{"ext": [".md", "txt"]}]))
        self.assertEqual([association.ext for association in associations], ["md", "txt"])

    def test_an_association_without_extensions_adds_nothing(self) -> None:
        associations = GENERATOR.get_associations(self.conf([{"ext": []}, {"ext": ["md"]}]))
        self.assertEqual([association.ext for association in associations], ["md"])

    def test_a_repeated_extension_is_rejected(self) -> None:
        with self.assertRaisesRegex(GENERATOR.GenerationError, "appears in"):
            GENERATOR.get_associations(self.conf([{"ext": ["md"]}, {"ext": ["MD"]}]))

    def test_an_extension_that_cannot_name_a_key_is_rejected(self) -> None:
        for ext in ("a\\b", "a b", "[x]", 'a"b', "", "."):
            with self.subTest(ext=ext):
                with self.assertRaises(GENERATOR.GenerationError):
                    GENERATOR.get_associations(self.conf([{"ext": [ext]}]))

    def test_a_missing_product_name_is_rejected(self) -> None:
        with self.assertRaisesRegex(GENERATOR.GenerationError, "productName"):
            GENERATOR.get_product_name({"productName": " "})

    def test_no_associations_is_rejected(self) -> None:
        with self.assertRaises(GENERATOR.GenerationError):
            GENERATOR.get_associations(self.conf([]))
        with self.assertRaises(GENERATOR.GenerationError):
            GENERATOR.render_fragment("Jotra", [])

    def test_a_description_is_escaped_for_xml_and_formatted_strings(self) -> None:
        associations = GENERATOR.get_associations(self.conf([{"ext": ["md"], "description": 'Notes [draft] "x" & {y}'}]))
        document = GENERATOR.render_fragment("Jotra", associations)
        key = find_one(ET.fromstring(document), "RegistryKey", Key=f"Software\\Classes\\{LEGACY_NAME}.md")
        self.assertEqual(default_value_of(key), 'Notes [\\[]draft[\\]] "x" & [\\{]y[\\}]')
        self.assertNotIn("{{", GENERATOR.render_fragment("{{x}}", associations))

    def test_the_open_command_names_the_main_binary_by_long_path(self) -> None:
        associations = GENERATOR.get_associations(self.conf([{"ext": ["md"]}]))
        root = ET.fromstring(GENERATOR.render_fragment("Jotra", associations))
        command = find_one(root, "RegistryKey", Key="command")
        self.assertEqual(default_value_of(command), '"[#Path]" "%1"')

    def test_the_first_progid_carries_the_key_path(self) -> None:
        associations = GENERATOR.get_associations(self.conf([{"ext": ["md", "txt"]}]))
        root = ET.fromstring(GENERATOR.render_fragment("Jotra", associations))
        key_paths = [value for value in root.iter(tag("RegistryValue")) if value.get("KeyPath") == "yes"]
        self.assertEqual(len(key_paths), 1)
        first = find_one(root, "RegistryKey", Key=f"Software\\Classes\\{LEGACY_NAME}.md")
        self.assertIn(key_paths[0], list(first.iter(tag("RegistryValue"))))


if __name__ == "__main__":
    unittest.main()
