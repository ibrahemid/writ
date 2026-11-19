"""Structural checks on release.yml and upgrade-checks.yml: which jobs see which secrets and tokens.

Run from the repository root:
    python3 -m unittest discover -s .github/scripts -t .github/scripts
"""

from __future__ import annotations

import fnmatch
import json
import os
import pathlib
import re
import shutil
import subprocess
import tempfile
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
RELEASE = REPO_ROOT / ".github/workflows/release.yml"
CI = REPO_ROOT / ".github/workflows/ci.yml"
UPGRADE_CHECKS = REPO_ROOT / ".github/workflows/upgrade-checks.yml"

SIGNING_JOBS = {"signing-config", "sign-macos", "sign-windows", "sign-updater"}
PLAN_REF = "ref: refs/tags/${{ needs.plan.outputs.tag_name }}"
CACHE_ACTIONS = ("Swatinem/rust-cache@", "actions/cache@", "actions/cache/restore@", "actions/cache/save@")
UPGRADE_SCRIPTS = {
    "deb-upgrade": "test_deb_upgrade_legacy.py",
    "deb-desktop-upgrade": "test_deb_upgrade_desktop.py",
    "install-sh-upgrade": "test_install_sh_upgrade.py",
    "aur-upgrade": "test_aur_writ_bin.py",
    "msi-upgrade": "test_msi_upgrade_legacy.py",
    "macos-upgrade": "test_macos_upgrade_legacy.py",
}
NEW_UPGRADE_JOBS = set(UPGRADE_SCRIPTS) - {"deb-upgrade"}
PREVIOUS_RELEASE_LINES = (
    "GH_TOKEN: ${{ github.token }}",
    "SOURCE_REPOSITORY_ID: '1278853784'",
    "PREVIOUS_TAG: v1.0.1",
    'SOURCE="$(gh api "repositories/$SOURCE_REPOSITORY_ID" --jq .full_name)"',
)


class WorkflowShapeError(LookupError):
    pass


def without_comments(text: str) -> str:
    return "\n".join(line for line in text.splitlines() if not line.lstrip().startswith("#"))


def workflow_jobs(text: str) -> dict[str, str]:
    lines = text.splitlines()
    if "jobs:" not in lines:
        raise WorkflowShapeError("the workflow has no jobs: block")
    jobs: dict[str, list[str]] = {}
    current: list[str] | None = None
    for line in lines[lines.index("jobs:") + 1 :]:
        header = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", line)
        if header:
            current = jobs.setdefault(header.group(1), [])
        elif line and not line[0].isspace() and not line.startswith("#"):
            break
        elif current is not None:
            current.append(line)
    return {name: without_comments("\n".join(body)) for name, body in jobs.items()}


def job_field(job: str, key: str) -> str | None:
    match = re.search(rf"^    {re.escape(key)}:(.*)$", job, re.MULTILINE)
    return match.group(1).strip() if match else None


def block_permissions(lines: list[str], indent: int) -> dict[str, str] | None:
    prefix = " " * indent
    for index, line in enumerate(lines):
        if line.startswith(f"{prefix}permissions:"):
            inline = line.split(":", 1)[1].strip()
            if inline == "{}":
                return {}
            if inline:
                raise WorkflowShapeError(f"permissions given as {inline!r}; list each scope")
            scopes: dict[str, str] = {}
            for body in lines[index + 1 :]:
                entry = re.match(rf"^{prefix}  ([a-z-]+): (read|write|none)$", body)
                if not entry:
                    break
                scopes[entry.group(1)] = entry.group(2)
            return scopes
    return None


def workflow_permissions(text: str) -> dict[str, str]:
    scopes = block_permissions(text.split("\njobs:\n", 1)[0].splitlines(), 0)
    if scopes is None:
        raise WorkflowShapeError("the workflow sets no top-level permissions")
    return scopes


def job_needs(job: str) -> list[str]:
    lines = job.splitlines()
    for index, line in enumerate(lines):
        match = re.match(r"^    needs:(.*)$", line)
        if not match:
            continue
        inline = match.group(1).strip()
        if inline.startswith("["):
            return [name.strip() for name in inline.strip("[]").split(",") if name.strip()]
        if inline:
            return [inline]
        names = []
        for body in lines[index + 1 :]:
            entry = re.match(r"^      - ([A-Za-z0-9_-]+)$", body)
            if not entry:
                break
            names.append(entry.group(1))
        return names
    return []


def job_condition(job: str) -> str:
    lines = job.splitlines()
    for index, line in enumerate(lines):
        match = re.match(r"^    if:(.*)$", line)
        if not match:
            continue
        inline = match.group(1).strip()
        if inline not in {">-", ">", "|"}:
            return inline
        body = []
        for text in lines[index + 1 :]:
            if not text.startswith("      "):
                break
            body.append(text.strip())
        return " ".join(body)
    return ""


def job_permissions(job: str, default: dict[str, str]) -> dict[str, str]:
    scopes = block_permissions(job.splitlines(), 4)
    return default if scopes is None else scopes


def job_steps(job: str) -> list[str]:
    lines = job.splitlines()
    if "    steps:" not in lines:
        return []
    steps: list[list[str]] = []
    for line in lines[lines.index("    steps:") + 1 :]:
        if re.match(r"^    \S", line):
            break
        if line.startswith("      - "):
            steps.append([line])
        elif steps:
            steps[-1].append(line)
    return ["\n".join(step) for step in steps]


def step_uses(step: str) -> str | None:
    match = re.search(r"^      (?:- |  )uses: (\S+)", step, re.MULTILINE)
    return match.group(1) if match else None


def step_script(step: str) -> str:
    lines = step.splitlines()
    for index, line in enumerate(lines):
        if line.strip() == "run: |":
            indent = len(line) - len(line.lstrip()) + 2
            body = []
            for text in lines[index + 1 :]:
                if text.strip() and len(text) - len(text.lstrip()) < indent:
                    break
                body.append(text[indent:])
            return "\n".join(body).rstrip() + "\n"
    raise WorkflowShapeError(f"no run block in {lines[0].strip()!r}")


def secret_names(text: str) -> set[str]:
    return set(re.findall(r"secrets\.([A-Za-z0-9_]+)", text)) - {"GITHUB_TOKEN"}


def sparse_paths(step: str) -> list[str]:
    lines = step.splitlines()
    for index, line in enumerate(lines):
        if line.strip() == "sparse-checkout: |":
            indent = len(lines[index + 1]) - len(lines[index + 1].lstrip())
            paths = []
            for body in lines[index + 1 :]:
                if not body.strip() or len(body) - len(body.lstrip()) < indent:
                    break
                paths.append(body.strip())
            return paths
    return []


def artifact_name(step: str) -> str:
    match = re.search(r"(?m)^          name: (\S+)$", step)
    if not match:
        raise WorkflowShapeError(f"no artifact name in {step.splitlines()[0].strip()!r}")
    return match.group(1)


def workflow_triggers(text: str) -> str:
    match = re.search(r"(?ms)^on:\n(.*?)^(?=\S)", text)
    if not match:
        raise WorkflowShapeError("the workflow has no on: block")
    return match.group(1)


def trigger_paths(triggers: str) -> list[str]:
    match = re.search(r"(?m)^    paths:\n((?:      - .*\n)+)", triggers)
    return [line.strip().removeprefix("- ") for line in match.group(1).splitlines()] if match else []


class ReleaseWorkflowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.text = RELEASE.read_text()
        self.jobs = workflow_jobs(self.text)
        self.default = workflow_permissions(self.text)

    def steps_using(self, job: str, action: str) -> list[str]:
        return [step for step in job_steps(self.jobs[job]) if (step_uses(step) or "").startswith(f"{action}@")]

    def test_the_parser_sees_every_job(self) -> None:
        self.assertEqual(
            set(self.jobs),
            {
                "plan",
                "build",
                "sbom",
                "signing-config",
                "sign-macos",
                "sign-windows",
                "sign-updater",
                "attest-sboms",
                "deb-upgrade",
                "deb-desktop-upgrade",
                "install-sh-upgrade",
                "aur-upgrade",
                "msi-upgrade",
                "macos-upgrade",
                "publish",
            },
        )
        for name, job in self.jobs.items():
            with self.subTest(job=name):
                self.assertTrue(job_steps(job), f"{name} has no steps the parser can read")

    def test_the_workflow_token_only_reads_unless_a_job_says_otherwise(self) -> None:
        self.assertEqual(self.default, {"contents": "read"})

    def test_secrets_are_read_only_inside_the_release_environment(self) -> None:
        for name, job in self.jobs.items():
            with self.subTest(job=name):
                if secret_names(job):
                    self.assertEqual(job_field(job, "environment"), "release", sorted(secret_names(job)))

    def test_the_release_environment_holds_only_the_signing_jobs(self) -> None:
        in_environment = {name for name, job in self.jobs.items() if job_field(job, "environment") == "release"}
        self.assertEqual(in_environment, SIGNING_JOBS)

    def test_each_secret_reaches_only_the_job_that_signs_with_it(self) -> None:
        owners = {"TAURI_SIGNING_": {"sign-updater"}, "APPLE_": {"sign-macos"}, "AZURE_": {"signing-config", "sign-windows"}}
        for prefix, allowed in owners.items():
            with self.subTest(prefix=prefix):
                readers = {
                    name for name, job in self.jobs.items() if any(secret.startswith(prefix) for secret in secret_names(job))
                }
                self.assertEqual(readers, allowed)
        everything = set().union(*(secret_names(job) for job in self.jobs.values()))
        self.assertTrue(all(any(secret.startswith(prefix) for prefix in owners) for secret in everything), everything)
        self.assertNotIn("WINDOWS_CERTIFICATE", self.text)

    def test_no_dependency_code_builds_or_restores_a_cache_beside_a_key(self) -> None:
        forbidden = ("cargo build", "cargo test", "cargo cyclonedx", "tauri build", "pnpm build", "npm install", "npx")
        for name in SIGNING_JOBS:
            job = self.jobs[name]
            with self.subTest(job=name):
                for command in forbidden:
                    self.assertIsNone(re.search(rf"(?<![\w-]){command}\b", job), f"{name} runs {command}")
                for step in job_steps(job):
                    uses = step_uses(step) or ""
                    self.assertFalse(uses.startswith(("Swatinem/rust-cache@", "actions/cache@")), uses)
                    if uses.startswith("actions/setup-node@"):
                        self.assertIn("package-manager-cache: false", step)
                        self.assertNotRegex(step, r"^\s+cache:", step)
                for install in re.findall(r"pnpm install[^\n]*", job):
                    self.assertIn("--ignore-scripts", install)

    def test_the_updater_key_job_checks_out_scripts_and_the_config_only(self) -> None:
        job = self.jobs["sign-updater"]
        [checkout] = self.steps_using("sign-updater", "actions/checkout")
        self.assertEqual(
            sparse_paths(checkout), [".github/scripts", "scripts/install-minisign.sh", "src-tauri/tauri.conf.json"]
        )
        self.assertIn("sparse-checkout-cone-mode: false", checkout)
        for tool in ("pnpm", "cargo", "npm", "npx", "node", "setup-node", "rustup"):
            self.assertIsNone(re.search(rf"(?<![\w-]){tool}\b", job), f"sign-updater runs {tool}")
        self.assertIn('bash scripts/install-minisign.sh "$RUNNER_TEMP/minisign"', job)

    def test_only_publish_writes_contents_and_only_attesting_jobs_mint_id_tokens(self) -> None:
        for name, job in self.jobs.items():
            scopes = job_permissions(job, self.default)
            with self.subTest(job=name):
                self.assertEqual(scopes.get("contents", "none") == "write", name == "publish", scopes)
                minted = scopes.get("id-token") == "write" or scopes.get("attestations") == "write"
                self.assertEqual(minted, name in {"publish", "attest-sboms"}, scopes)
                self.assertTrue(set(scopes) <= {"contents", "attestations", "id-token"}, scopes)
        self.assertEqual(job_permissions(self.jobs["signing-config"], self.default), {})

    def test_every_checkout_keeps_no_credentials_and_reads_the_planned_tag(self) -> None:
        for name in self.jobs:
            for step in self.steps_using(name, "actions/checkout"):
                with self.subTest(job=name):
                    self.assertIn("persist-credentials: false", step)
                    if name == "plan":
                        self.assertIn("ref: refs/tags/${{ steps.tag.outputs.tag }}", step)
                    else:
                        self.assertIn(PLAN_REF, step)

    def test_plan_checks_the_tag_before_it_checks_out_or_runs_repository_code(self) -> None:
        steps = job_steps(self.jobs["plan"])
        resolve = next(i for i, step in enumerate(steps) if step.startswith("      - name: Resolve tag"))
        checkout = next(i for i, step in enumerate(steps) if (step_uses(step) or "").startswith("actions/checkout@"))
        runs_code = [i for i, step in enumerate(steps) if re.search(r"\b(node|python3) (scripts|\.github)/", step)]
        self.assertTrue(runs_code)
        self.assertEqual(resolve, 0, "the tag check is the plan job's first step")
        self.assertLess(checkout, min(runs_code))
        self.assertNotIn("${{", step_script(steps[resolve]), "the tag check reads its inputs from env only")

    def test_no_job_restores_or_saves_an_actions_cache(self) -> None:
        for name, job in self.jobs.items():
            for step in job_steps(job):
                uses = step_uses(step) or ""
                with self.subTest(job=name, step=step.splitlines()[0].strip()):
                    self.assertFalse(uses.startswith(CACHE_ACTIONS), f"{name} uses {uses}: release builds start cold")
                    if uses.startswith("actions/setup-node@"):
                        self.assertIn("package-manager-cache: false", step)
                        self.assertNotRegex(step, r"(?m)^\s+cache(-dependency-path)?:")
                    if uses.startswith("pnpm/action-setup@"):
                        self.assertNotRegex(step, r"(?m)^\s+cache:\s*['\"]?true")

    def test_the_build_job_holds_no_secret_and_signs_nothing(self) -> None:
        job = self.jobs["build"]
        self.assertEqual(secret_names(job), set())
        self.assertIsNone(job_field(job, "environment"))
        self.assertIn("""--config '{"bundle":{"createUpdaterArtifacts":false}}'""", job)
        self.assertIn('APPLE_SIGNING_IDENTITY: "-"', job)
        self.assertNotIn("TAURI_SIGNING", job)

    def test_every_action_is_pinned_to_a_commit(self) -> None:
        uses = re.findall(r"uses: (\S+)", self.text)
        self.assertTrue(uses)
        for reference in uses:
            with self.subTest(uses=reference):
                self.assertRegex(reference, r"^[\w.-]+/[\w./-]+@[0-9a-f]{40}$")

    def test_windows_signing_runs_only_with_an_account_and_never_blocks_a_release_without_one(self) -> None:
        self.assertEqual(job_field(self.jobs["sign-windows"], "if"), "needs.signing-config.outputs.windows == 'true'")
        config = self.jobs["signing-config"]
        for secret in ("AZURE_TENANT_ID", "AZURE_CLIENT_ID", "AZURE_CLIENT_SECRET"):
            self.assertIn(f"secrets.{secret} != ''", config)
        gate = " ".join(self.jobs["sign-updater"].split("    runs-on:")[0].split())
        self.assertIn("needs.sign-windows.result == 'success'", gate)
        self.assertIn("(needs.sign-windows.result == 'skipped' && needs.signing-config.outputs.windows != 'true')", gate)
        self.assertIn("!cancelled()", gate)

    def test_the_legacy_launcher_is_built_carried_and_signed_before_each_msi(self) -> None:
        launcher = "src-tauri/binaries/writ-tauri.exe"
        fragment = (REPO_ROOT / "src-tauri/wix/legacy-install.wxs").read_text()
        self.assertIn(r'Source="$(sys.SOURCEFILEDIR)..\binaries\writ-tauri.exe"', fragment)

        build = job_steps(self.jobs["build"])
        [built] = [step for step in build if "cargo build -p writ-tauri" in step]
        self.assertIn("if: matrix.os == 'windows-latest'", built)
        script = step_script(built)
        self.assertIn("cargo build -p writ-tauri --release --locked --target x86_64-pc-windows-msvc", script)
        self.assertIn(f"cp target/x86_64-pc-windows-msvc/release/writ-tauri.exe {launcher}", script)
        [bundled] = [step for step in build if "pnpm exec tauri build" in step]
        [staged] = [step for step in build if step.splitlines()[0].strip() == "- name: Stage the build for signing"]
        self.assertLess(build.index(built), build.index(bundled))
        self.assertIn(f"cp {launcher} dist/build-windows-binaries/", step_script(staged))

        sign = job_steps(self.jobs["sign-windows"])
        [restored] = [step for step in sign if "build-windows-binaries/jotra-tauri.exe" in step]
        self.assertIn("cp build-windows-binaries/writ-tauri.exe src-tauri/binaries/", step_script(restored))
        [config] = [step for step in sign if "--out-dir \"$RUNNER_TEMP/windows-signing\"" in step]
        [signed] = [step for step in sign if "windows_signing.py sign " in step]
        [rebundled] = [step for step in sign if "pnpm exec tauri bundle" in step]
        signing = step_script(signed)
        self.assertIn(f'$launcher = "{launcher}"', signing)
        self.assertIn("windows-signing/tauri.windows-signing.conf.json\" $launcher", signing)
        self.assertIn("Get-AuthenticodeSignature -FilePath $launcher", signing)
        for secret in ("AZURE_TENANT_ID", "AZURE_CLIENT_ID", "AZURE_CLIENT_SECRET"):
            self.assertIn(f"{secret}: ${{{{ secrets.{secret} }}}}", signed)
        self.assertNotRegex(signed, r"(?m)^        if:")
        order = [sign.index(step) for step in (restored, config, signed, rebundled)]
        self.assertEqual(order, sorted(order))

    def test_publish_checks_the_set_then_attests_every_asset(self) -> None:
        job = self.jobs["publish"]
        needs = job_needs(job)
        for upstream in ("sign-updater", "attest-sboms"):
            self.assertIn(upstream, needs)
        steps = job_steps(job)
        check = next(i for i, step in enumerate(steps) if "--set published" in step)
        attest = next(i for i, step in enumerate(steps) if (step_uses(step) or "").startswith("actions/attest@"))
        upload = next(i for i, step in enumerate(steps) if "gh release upload" in step)
        self.assertLess(check, attest)
        self.assertLess(attest, upload)
        for index in (check, attest, upload):
            self.assertNotRegex(steps[index], r"(?m)^        if:", steps[index].splitlines()[0])
        self.assertIn("subject-path: release-assets/*", steps[attest])
        [sbom] = self.steps_using("attest-sboms", "actions/attest")
        self.assertIn("sbom-path: sboms/${{ matrix.sbom }}", sbom)
        self.assertIn("subject-checksums: subjects.sha256", sbom)

    def test_the_sums_signature_ships_with_the_checksums(self) -> None:
        job = self.jobs["sign-updater"]
        self.assertIn("release_signing.py sign-sums", job)
        self.assertIn("release_signing.py verify-release", job)
        self.assertIn("release-assets/SHA256SUMS.txt.minisig", job)

    def test_the_sboms_are_read_before_upload_and_keep_their_serial_numbers(self) -> None:
        job = self.jobs["sbom"]
        self.assertFalse(
            "SOURCE_DATE_EPOCH" in job,
            "cargo-cyclonedx leaves serialNumber out under SOURCE_DATE_EPOCH, and actions/attest refuses such an SBOM",
        )
        steps = job_steps(job)
        generate = next(i for i, step in enumerate(steps) if "cargo cyclonedx" in step)
        upload = next(i for i, step in enumerate(steps) if (step_uses(step) or "").startswith("actions/upload-artifact@"))
        script = steps[generate]
        check = "release_assets.py check --dir dist/sboms --version \"$VERSION\" --set sboms"
        self.assertIn(check, script)
        self.assertLess(script.index("cargo cyclonedx"), script.index(check))
        self.assertLess(script.index("cdxgen\" -t js"), script.index(check))
        self.assertLess(generate, upload)

    def test_each_build_files_its_debug_symbols_in_an_artifact_no_other_job_reads(self) -> None:
        steps = job_steps(self.jobs["build"])
        collect = [step for step in steps if "debug_symbols.py" in step]
        self.assertEqual(len(collect), 1)
        script = step_script(collect[0])
        for kind in ("dsym", "pdb", "elf"):
            self.assertIn(f"debug_symbols.py {kind} --out", script)
        uploads = [step for step in self.steps_using("build", "actions/upload-artifact") if "debug-symbols-" in step]
        self.assertEqual(len(uploads), 1)
        self.assertNotIn("if:", uploads[0], "every platform keeps its symbols")
        self.assertLess(steps.index(collect[0]), steps.index(uploads[0]))
        for name, job in self.jobs.items():
            if name != "build":
                with self.subTest(job=name):
                    self.assertNotIn("debug-symbols", job)

    def test_the_linux_leg_splits_its_symbols_before_anything_reads_the_bundles(self) -> None:
        names = [step.splitlines()[0].strip().removeprefix("- name: ") for step in job_steps(self.jobs["build"])]
        order = [
            "Keep the Linux line tables for the symbol split",
            "Build Tauri app",
            "Split off the Linux debug symbols and bundle again",
            "Require every AppImage file to be readable and runnable by any user",
            "Add the legacy entries to the deb",
            "Stage the build for signing",
        ]
        self.assertEqual([name for name in names if name in order], order)

    def test_the_deb_gets_its_legacy_entries_before_it_is_staged(self) -> None:
        [step] = [step for step in job_steps(self.jobs["build"]) if "deb_legacy_entries.py" in step]
        self.assertIn("if: matrix.os == 'ubuntu-22.04'", step)
        self.assertIn(
            'python3 scripts/deb_legacy_entries.py "target/x86_64-unknown-linux-gnu/release/bundle/deb/Jotra_${VERSION}_amd64.deb"',
            step_script(step),
        )

    def test_the_new_deb_installs_over_the_previous_release_before_publish(self) -> None:
        job = self.jobs["deb-upgrade"]
        self.assertEqual(job_needs(job), ["plan", "build"])
        self.assertEqual(job_field(job, "runs-on"), "ubuntu-22.04")
        steps = job_steps(job)
        download = next(
            i
            for i, step in enumerate(steps)
            if (step_uses(step) or "").startswith("actions/download-artifact@") and "name: build-linux" in step
        )
        previous = next(i for i, step in enumerate(steps) if "gh release download" in step)
        check = next(i for i, step in enumerate(steps) if "test_deb_upgrade_legacy.py" in step)
        self.assertLess(download, check)
        self.assertLess(previous, check)
        self.assertIn("PREVIOUS_TAG: v1.0.1", steps[previous])
        script = step_script(steps[check])
        self.assertIn('export DEB_UPGRADE_TO="build-linux/Jotra_${VERSION}_amd64.deb"', script)
        self.assertIn("python3 .github/scripts/test_deb_upgrade_legacy.py", script)
        publish = self.jobs["publish"]
        self.assertIn("deb-upgrade", job_needs(publish))
        self.assertIn("needs.deb-upgrade.result == 'success'", job_condition(publish))

    def test_the_notarization_requirement_follows_the_repository_id(self) -> None:
        self.assertIn("REQUIRED: ${{ github.repository_id == '1278853784' }}", self.jobs["sign-macos"])
        self.assertNotRegex(self.text, r"github\.repository\s*[!=]=")
        [previous] = [step for step in job_steps(self.jobs["deb-upgrade"]) if "gh release download" in step]
        self.assertIn("SOURCE_REPOSITORY_ID: '1278853784'", previous)

    def test_runs_for_one_tag_queue_instead_of_racing(self) -> None:
        block = re.search(r"(?m)^concurrency:\n((?:  .*\n)+)", self.text)
        self.assertIsNotNone(block, "release.yml has no workflow-level concurrency group")
        body = block.group(1)
        self.assertIn(
            "group: release-${{ github.event_name == 'workflow_dispatch' && inputs.tag || github.ref_name }}", body
        )
        self.assertIn("cancel-in-progress: false", body)

    def test_every_sparse_checkout_pattern_is_anchored(self) -> None:
        for name in self.jobs:
            for step in self.steps_using(name, "actions/checkout"):
                for pattern in sparse_paths(step):
                    with self.subTest(job=name, pattern=pattern):
                        self.assertTrue(
                            pattern.startswith("/") or "/" in pattern.rstrip("/"),
                            f"{pattern} also matches every file of that name below the root",
                        )

    def test_every_upgrade_check_gates_publish(self) -> None:
        publish = self.jobs["publish"]
        condition = job_condition(publish)
        self.assertTrue(condition.startswith("${{ !cancelled()"), condition)
        for name in UPGRADE_SCRIPTS:
            with self.subTest(job=name):
                self.assertIn(name, job_needs(publish))
                self.assertIn(f"needs.{name}.result == 'success'", condition)

    def test_the_upgrade_checks_read_contents_only_and_hold_no_secret(self) -> None:
        for name in UPGRADE_SCRIPTS:
            job = self.jobs[name]
            with self.subTest(job=name):
                self.assertIsNone(job_field(job, "environment"))
                self.assertEqual(secret_names(job), set())
                self.assertNotIn("TAURI_SIGNING", job)
                self.assertEqual(job_permissions(job, self.default), {"contents": "read"})
        for name in NEW_UPGRADE_JOBS:
            with self.subTest(job=name):
                self.assertEqual(block_permissions(self.jobs[name].splitlines(), 4), {"contents": "read"})

    def test_each_upgrade_check_reads_the_previous_release_by_repository_id(self) -> None:
        for name in UPGRADE_SCRIPTS:
            steps = [step for step in job_steps(self.jobs[name]) if "gh release download" in step]
            with self.subTest(job=name):
                self.assertEqual(len(steps), 1)
                for line in PREVIOUS_RELEASE_LINES:
                    self.assertIn(line, steps[0])
                self.assertNotIn("github.repository", steps[0])

    def test_each_upgrade_check_runs_its_script_after_its_downloads(self) -> None:
        for name, script in UPGRADE_SCRIPTS.items():
            steps = job_steps(self.jobs[name])
            with self.subTest(job=name):
                checks = [i for i, step in enumerate(steps) if f".github/scripts/{script}" in step]
                self.assertEqual(len(checks), 1)
                downloads = [
                    i
                    for i, step in enumerate(steps)
                    if (step_uses(step) or "").startswith("actions/download-artifact@") or "gh release download" in step
                ]
                self.assertTrue(downloads)
                self.assertLess(max(downloads), checks[0])

    def test_the_deb_desktop_check_updates_the_built_deb(self) -> None:
        job = self.jobs["deb-desktop-upgrade"]
        self.assertEqual(job_needs(job), ["plan", "build"])
        self.assertEqual(job_field(job, "runs-on"), "ubuntu-22.04")
        [download] = self.steps_using("deb-desktop-upgrade", "actions/download-artifact")
        self.assertEqual(artifact_name(download), "build-linux")
        [check] = [step for step in job_steps(job) if "test_deb_upgrade_desktop.py" in step]
        self.assertIn('export DEB_UPGRADE_TO="build-linux/Jotra_${VERSION}_amd64.deb"', step_script(check))

    def test_the_macos_check_replaces_the_signed_app_for_both_owners(self) -> None:
        job = self.jobs["macos-upgrade"]
        self.assertEqual(job_needs(job), ["plan", "sign-macos"])
        self.assertEqual(
            job_condition(job), "${{ !cancelled() && needs.plan.result == 'success' && needs.sign-macos.result == 'success' }}"
        )
        self.assertEqual(job_field(job, "runs-on"), "macos-latest")
        self.assertIn("- owner: user\n            previous: '*_universal.app.tar.gz'", job)
        self.assertIn("- owner: admin\n            previous: '*_universal.pkg'", job)
        [download] = self.steps_using("macos-upgrade", "actions/download-artifact")
        self.assertEqual(artifact_name(download), "signed-macos")
        [check] = [step for step in job_steps(job) if "test_macos_upgrade_legacy.py" in step]
        self.assertIn("MACOS_UPGRADE_OWNER: ${{ matrix.owner }}", check)
        self.assertIn('export MACOS_UPGRADE_TO="signed-macos/Jotra_${VERSION}_universal.app.tar.gz"', step_script(check))

    def test_the_msi_check_installs_the_msi_that_ships_into_both_folders(self) -> None:
        job = self.jobs["msi-upgrade"]
        self.assertEqual(job_needs(job), ["plan", "build", "signing-config", "sign-windows"])
        self.assertEqual(job_field(job, "runs-on"), "windows-latest")
        self.assertIn("install_dir: [default, custom]", job)
        condition = job_condition(job)
        self.assertTrue(condition.startswith("${{ !cancelled()"), condition)
        self.assertIn(
            "(needs.sign-windows.result == 'skipped' && needs.signing-config.outputs.windows != 'true')", condition
        )
        downloads = {artifact_name(step): step for step in self.steps_using("msi-upgrade", "actions/download-artifact")}
        self.assertEqual(set(downloads), {"signed-windows", "build-windows"})
        self.assertIn("if: needs.sign-windows.result == 'success'", downloads["signed-windows"])
        self.assertIn("if: needs.sign-windows.result != 'success'", downloads["build-windows"])
        [checkout] = self.steps_using("msi-upgrade", "actions/checkout")
        self.assertIn("src-tauri/wix/legacy-progids.wxs", sparse_paths(checkout))
        [check] = [step for step in job_steps(job) if "test_msi_upgrade_legacy.py" in step]
        script = step_script(check)
        self.assertIn("$env:MSI_UPGRADE_INSTALL_DIR = ", script)
        self.assertIn("if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }", script)

    def test_the_install_sh_and_aur_checks_install_the_signed_appimage(self) -> None:
        for name, folder in (("install-sh-upgrade", "install-assets"), ("aur-upgrade", "aur-assets")):
            job = self.jobs[name]
            with self.subTest(job=name):
                self.assertEqual(job_needs(job), ["plan", "build", "sign-updater"])
                self.assertEqual(
                    job_condition(job),
                    "${{ !cancelled() && needs.plan.result == 'success' && needs.build.result == 'success'"
                    " && needs.sign-updater.result == 'success' }}",
                )
                downloads = self.steps_using(name, "actions/download-artifact")
                self.assertEqual(sorted(artifact_name(step) for step in downloads), ["build-linux", "signed-updater"])
                for step in downloads:
                    self.assertIn(f"path: {folder}", step)
        install = job_steps(self.jobs["install-sh-upgrade"])
        [previous] = [step for step in install if "gh release download" in step]
        self.assertIn('"repos/$SOURCE/contents/install.sh?ref=$PREVIOUS_TAG"', previous)
        minisign = next(i for i, step in enumerate(install) if "- name: Install minisign" in step)
        tests = next(i for i, step in enumerate(install) if "test_install_sh*.py" in step)
        self.assertLess(minisign, tests)
        self.assertIn("MINISIGN: ${{ steps.minisign.outputs.path }}", install[tests])
        self.assertIn("python3 -m unittest discover -v -s scripts -p 'test_install_sh*.py'", install[tests])
        [aur] = [step for step in job_steps(self.jobs["aur-upgrade"]) if "gh release download" in step]
        self.assertIn('"repos/$SOURCE/contents/packaging/aur/writ-bin/PKGBUILD?ref=$PREVIOUS_TAG"', aur)


class DispatchedTagTests(unittest.TestCase):
    """Runs plan's Resolve tag script, verbatim from release.yml, against a local origin."""

    @classmethod
    def setUpClass(cls) -> None:
        steps = job_steps(workflow_jobs(RELEASE.read_text())["plan"])
        [step] = [step for step in steps if step.startswith("      - name: Resolve tag")]
        cls.script = step_script(step)

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="jotra-release-tag-test-")
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.env = {
            "PATH": os.environ["PATH"],
            "HOME": str(self.root),
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_AUTHOR_NAME": "Release Test",
            "GIT_AUTHOR_EMAIL": "release-test@example.invalid",
            "GIT_COMMITTER_NAME": "Release Test",
            "GIT_COMMITTER_EMAIL": "release-test@example.invalid",
        }
        work = self.root / "work"
        self.git("init", "--quiet", "--initial-branch=main", str(work))
        self.commit(work, "first")
        self.git("-C", str(work), "tag", "v0.5.0")
        self.commit(work, "second")
        self.git("-C", str(work), "tag", "--annotate", "--message", "Jotra 0.5.1", "v0.5.1")
        self.git("-C", str(work), "branch", "v9.9.9")
        self.git("-C", str(work), "branch", "v0.6.0-1")
        self.git("-C", str(work), "tag", "v0.7.0", "HEAD^{tree}")
        self.git("-C", str(work), "switch", "--quiet", "--create", "unmerged", "v0.5.0")
        self.commit(work, "never merged")
        self.git("-C", str(work), "tag", "v0.6.0-1")
        self.unmerged = self.git("-C", str(work), "rev-parse", "HEAD").stdout.strip()
        self.git("clone", "--quiet", "--bare", str(work), str(self.root / "server" / "ibrahemid" / "writ.git"))

    def git(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(["git", *args], env=self.env, capture_output=True, text=True, check=True)

    def commit(self, work: pathlib.Path, message: str) -> None:
        self.git("-C", str(work), "commit", "--quiet", "--allow-empty", "--message", message)

    def resolve(
        self, tag: str, event: str = "workflow_dispatch"
    ) -> tuple[subprocess.CompletedProcess[str], dict[str, str], pathlib.Path]:
        runner_temp = pathlib.Path(tempfile.mkdtemp(dir=self.root))
        output = runner_temp / "github_output"
        output.touch()
        script = runner_temp / "resolve.sh"
        script.write_text(self.script)
        dispatched = event == "workflow_dispatch"
        env = {
            **self.env,
            "EVENT_NAME": event,
            "INPUT_TAG": tag if dispatched else "",
            "REF_NAME": "main" if dispatched else tag,
            "DEFAULT_BRANCH": "main",
            "GITHUB_SERVER_URL": f"file://{self.root / 'server'}",
            "GITHUB_REPOSITORY": "ibrahemid/writ",
            "GITHUB_OUTPUT": str(output),
            "RUNNER_TEMP": str(runner_temp),
        }
        result = subprocess.run(
            ["bash", "--noprofile", "--norc", "-eo", "pipefail", str(script)],
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )
        outputs = dict(line.split("=", 1) for line in output.read_text().splitlines())
        return result, outputs, runner_temp

    def assert_refused(self, result: subprocess.CompletedProcess[str], outputs: dict[str, str], message: str) -> None:
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(f"::error::{message}", result.stdout)
        self.assertEqual(outputs, {})

    def test_a_tag_on_main_is_released(self) -> None:
        for tag in ("v0.5.0", "v0.5.1"):
            with self.subTest(tag=tag):
                result, outputs, _ = self.resolve(tag)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual(outputs, {"tag": tag, "version": tag[1:]})
                self.assertIn(f"{tag} (", result.stdout)
                self.assertIn(") is on main", result.stdout)

    def test_a_tag_whose_commit_is_not_on_main_is_refused(self) -> None:
        result, outputs, _ = self.resolve("v0.6.0-1")
        self.assert_refused(result, outputs, f"Tag v0.6.0-1 ({self.unmerged}) is not on main.")

    def test_a_branch_name_is_refused(self) -> None:
        result, outputs, runner_temp = self.resolve("main")
        self.assert_refused(result, outputs, "Refusing to release main:")
        self.assertFalse((runner_temp / "release-tag-check.git").exists())

    def test_a_branch_named_like_a_release_is_refused(self) -> None:
        result, outputs, _ = self.resolve("v9.9.9")
        self.assert_refused(result, outputs, "v9.9.9 is a branch, not a tag.")

    def test_a_missing_tag_is_refused(self) -> None:
        result, outputs, _ = self.resolve("v1.0.0")
        self.assert_refused(result, outputs, "No tag named v1.0.0 exists.")

    def test_a_tag_that_names_no_commit_is_refused(self) -> None:
        result, outputs, _ = self.resolve("v0.7.0")
        self.assert_refused(result, outputs, "Tag v0.7.0 does not point to a commit.")

    def test_revision_syntax_and_injected_text_are_refused_before_any_fetch(self) -> None:
        for tag in (
            "",
            "v0.5",
            "V0.5.1",
            "v0.5.1-",
            "v0.5.1^",
            "v0.5.1~1",
            "v0.5.1^{commit}",
            "v0.5.1:refs/heads/main",
            "refs/tags/v0.5.1",
            "v0.5.1 --upload-pack=touch",
            "v0.5.1\n::warning::injected",
        ):
            with self.subTest(tag=tag):
                result, outputs, runner_temp = self.resolve(tag)
                self.assert_refused(result, outputs, "Refusing to release ")
                self.assertEqual(len(result.stdout.splitlines()), 1, result.stdout)
                self.assertFalse((runner_temp / "release-tag-check.git").exists())

    def test_a_pushed_tag_resolves_without_fetching(self) -> None:
        result, outputs, runner_temp = self.resolve("v0.6.0-1", event="push")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(outputs, {"tag": "v0.6.0-1", "version": "0.6.0-1"})
        self.assertFalse((runner_temp / "release-tag-check.git").exists())


class UpgradeChecksWorkflowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.text = UPGRADE_CHECKS.read_text()
        self.jobs = workflow_jobs(self.text)
        self.default = workflow_permissions(self.text)
        self.release = workflow_jobs(RELEASE.read_text())

    def named_step(self, jobs: dict[str, str], job: str, name: str) -> str:
        steps = [step for step in job_steps(jobs[job]) if step.splitlines()[0].strip() == f"- name: {name}"]
        self.assertEqual(len(steps), 1, f"{job} has {len(steps)} steps named {name!r}")
        return steps[0]

    def test_it_runs_on_demand_and_for_pull_requests_that_touch_the_upgrade_path(self) -> None:
        triggers = workflow_triggers(self.text)
        self.assertEqual(re.findall(r"(?m)^  ([a-z_]+):", triggers), ["workflow_dispatch", "pull_request"])
        paths = trigger_paths(triggers)
        for required in (
            ".github/workflows/release.yml",
            ".github/workflows/upgrade-checks.yml",
            "packaging/**",
            "src-tauri/wix/**",
            "install.sh",
            "crates/writ-tauri/**",
            "scripts/deb_legacy_entries.py",
        ):
            with self.subTest(path=required):
                self.assertIn(required, paths)
        for script in UPGRADE_SCRIPTS.values():
            with self.subTest(script=script):
                self.assertTrue(any(fnmatch.fnmatchcase(f".github/scripts/{script}", pattern) for pattern in paths))
        for pattern in paths:
            with self.subTest(pattern=pattern):
                literal = re.split(r"[*?\[]", pattern, maxsplit=1)[0].rstrip("/")
                if literal == pattern:
                    self.assertTrue((REPO_ROOT / pattern).is_file(), f"{pattern} names no file")
                elif pattern.endswith("/**"):
                    self.assertTrue((REPO_ROOT / literal).is_dir(), f"{pattern} names no folder")
                else:
                    self.assertTrue(list(REPO_ROOT.glob(pattern)), f"{pattern} matches no file")

    def test_the_token_only_reads_and_no_secret_or_environment_is_named(self) -> None:
        self.assertEqual(self.default, {"contents": "read"})
        for name, job in self.jobs.items():
            with self.subTest(job=name):
                self.assertEqual(job_permissions(job, self.default), {"contents": "read"})
                self.assertIsNone(job_field(job, "environment"))
        self.assertNotIn("secrets.", self.text)

    def test_every_action_is_pinned_and_every_checkout_keeps_no_credentials(self) -> None:
        uses = re.findall(r"uses: (\S+)", self.text)
        self.assertTrue(uses)
        for reference in uses:
            with self.subTest(uses=reference):
                self.assertRegex(reference, r"^[\w.-]+/[\w./-]+@[0-9a-f]{40}$")
        for name, job in self.jobs.items():
            for step in job_steps(job):
                uses = step_uses(step) or ""
                with self.subTest(job=name, step=step.splitlines()[0].strip()):
                    self.assertFalse(uses.startswith(CACHE_ACTIONS), uses)
                    if uses.startswith("actions/checkout@"):
                        self.assertIn("persist-credentials: false", step)
                        for pattern in sparse_paths(step):
                            self.assertTrue(pattern.startswith("/") or "/" in pattern.rstrip("/"), pattern)

    def test_it_runs_every_upgrade_check_of_the_release(self) -> None:
        self.assertEqual(set(self.jobs), {"plan", "build"} | set(UPGRADE_SCRIPTS))
        for name, script in UPGRADE_SCRIPTS.items():
            with self.subTest(job=name):
                self.assertIn(f".github/scripts/{script}", self.jobs[name])
                self.assertIn("build", job_needs(self.jobs[name]))
        install = self.jobs["install-sh-upgrade"]
        self.assertIn("python3 -m unittest discover -v -s scripts -p 'test_install_sh*.py'", install)
        self.assertIn("install_dir: [default, custom]", self.jobs["msi-upgrade"])
        self.assertIn("- owner: admin\n            previous: '*_universal.pkg'", self.jobs["macos-upgrade"])

    def test_each_check_runs_when_its_own_build_leg_left_bundles(self) -> None:
        self.assertIn("    strategy:\n      fail-fast: false\n", self.jobs["build"])
        legs = {
            "upgrade-linux": "Linux (x64)",
            "upgrade-windows": "Windows (x64)",
            "upgrade-macos": "macOS (universal)",
        }
        for name in UPGRADE_SCRIPTS:
            job = self.jobs[name]
            with self.subTest(job=name):
                self.assertEqual(job_condition(job), "${{ !cancelled() && needs.plan.result == 'success' }}")
                steps = job_steps(job)
                downloads = [
                    step for step in steps if (step_uses(step) or "").startswith("actions/download-artifact@")
                ]
                self.assertEqual(downloads, steps[1:2])
                self.assertTrue((step_uses(steps[0]) or "").startswith("actions/checkout@"))
                download, listing = steps[1], steps[2]
                artifact = artifact_name(download)
                self.assertIn(artifact, legs)
                folder = re.search(r"(?m)^          path: (\S+)$", download).group(1)
                self.assertIn("        id: bundles\n        continue-on-error: true\n", download)
                self.assertIn("          DOWNLOAD: ${{ steps.bundles.outcome }}\n", listing)
                self.assertNotIn("        if:", listing)
                script = step_script(listing)
                self.assertIn('if [ "$DOWNLOAD" != success ]; then', script)
                self.assertIn(f"The Build ({legs[artifact]}) job of this run uploaded no {artifact} artifact", script)
                self.assertTrue(script.rstrip().endswith(f"ls -la {folder}"))

    def test_no_checkout_runs_after_a_download_it_would_delete(self) -> None:
        for workflow, jobs in (("upgrade-checks.yml", self.jobs), ("release.yml", self.release)):
            for name, job in jobs.items():
                uses = [step_uses(step) or "" for step in job_steps(job)]
                downloads = [i for i, action in enumerate(uses) if action.startswith("actions/download-artifact@")]
                checkouts = [i for i, action in enumerate(uses) if action.startswith("actions/checkout@")]
                if downloads and checkouts:
                    with self.subTest(workflow=workflow, job=name):
                        self.assertLess(max(checkouts), min(downloads))

    def test_it_reads_the_same_previous_release_as_the_release_workflow(self) -> None:
        for name in UPGRADE_SCRIPTS:
            mine = [step for step in job_steps(self.jobs[name]) if "gh release download" in step]
            release = [step for step in job_steps(self.release[name]) if "gh release download" in step]
            with self.subTest(job=name):
                self.assertEqual(mine, release)

    def test_the_build_runs_the_release_build_steps_the_checks_read(self) -> None:
        for name in (
            "Install Rust toolchain",
            "Install Linux dependencies",
            "Install frontend dependencies",
            "Build CLI sidecar",
            "Build the legacy launcher",
            "Seed AppRun with world-execute permission",
            "Legacy ProgId fragment matches the file associations",
            "Build Tauri app",
            "Add the legacy entries to the deb",
        ):
            with self.subTest(step=name):
                self.assertEqual(self.named_step(self.jobs, "build", name), self.named_step(self.release, "build", name))
        names = [step.splitlines()[0].strip().removeprefix("- name: ") for step in job_steps(self.jobs["build"])]
        order = [
            "Set the version to build",
            "Install frontend dependencies",
            "Build the legacy launcher",
            "Build Tauri app",
            "Add the legacy entries to the deb",
            "Stage the bundles",
            "Sign the AppImage and its checksums with a key made for this run",
            "Upload the bundles",
        ]
        self.assertEqual([name for name in names if name in order], order)
        bump = step_script(self.named_step(self.jobs, "build", "Set the version to build"))
        self.assertIn('"$PYTHON" .github/scripts/bump_version.py "$VERSION"', bump)
        self.assertIn("cargo update --workspace", bump)
        stage = step_script(self.named_step(self.jobs, "build", "Stage the bundles"))
        self.assertIn("python3 .github/scripts/archive_app.py create", stage)
        self.assertIn('"dist/upgrade-macos/Jotra_${VERSION}_universal.app.tar.gz"', stage)
        for bundles in ("bundles: app", "bundles: msi", "bundles: deb,appimage"):
            self.assertIn(f"            {bundles}\n", self.jobs["build"])

    def test_the_run_key_signs_the_appimage_and_stays_in_the_build_job(self) -> None:
        sign = self.named_step(self.jobs, "build", "Sign the AppImage and its checksums with a key made for this run")
        self.assertIn("if: matrix.os == 'ubuntu-22.04'", sign)
        script = step_script(sign)
        self.assertIn('KEY_DIR="$RUNNER_TEMP/upgrade-key"', script)
        self.assertIn('-G -W -p dist/upgrade-linux/minisign.pub -s "$KEY_DIR/upgrade.key"', script)
        self.assertIn("release_signing.py sign-payloads", script)
        self.assertLess(script.index("SHA256SUMS.txt.minisig"), script.index('rm -rf "$KEY_DIR"'))
        [upload] = self.steps_using_build("actions/upload-artifact")
        self.assertIn("path: dist/upgrade-${{ matrix.platform }}/", upload)
        self.assertEqual(self.text.count("upgrade.key"), 3)
        for name, variable in (("install-sh-upgrade", "INSTALL_SH_PUBKEY"), ("aur-upgrade", "AUR_MINISIGN_PUBKEY")):
            with self.subTest(job=name):
                self.assertIn(f'{variable}="$(sed -n 2p upgrade-linux/minisign.pub)"', self.jobs[name])

    def steps_using_build(self, action: str) -> list[str]:
        return [step for step in job_steps(self.jobs["build"]) if (step_uses(step) or "").startswith(f"{action}@")]

    def run_version_step(self, version: str) -> tuple[subprocess.CompletedProcess[str], str]:
        script = step_script(self.named_step(self.jobs, "plan", "Next patch version"))
        with tempfile.TemporaryDirectory(prefix="upgrade-version-") as scratch:
            work = pathlib.Path(scratch)
            (work / "src-tauri").mkdir()
            (work / "src-tauri/tauri.conf.json").write_text(json.dumps({"version": version}))
            output = work / "github-output"
            output.write_text("")
            result = subprocess.run(
                ["bash", "-c", script],
                cwd=work,
                env={**os.environ, "GITHUB_OUTPUT": str(output)},
                capture_output=True,
                text=True,
                check=False,
                timeout=30,
            )
            return result, output.read_text()

    def test_it_builds_the_next_patch_version(self) -> None:
        if not shutil.which("jq"):
            self.skipTest("jq is not installed")
        for current, expected in (("1.0.1", "1.0.2"), ("2.9.19", "2.9.20")):
            with self.subTest(version=current):
                result, output = self.run_version_step(current)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(output, f"version={expected}\n")
        result, output = self.run_version_step("1.0.2-rc.1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("prerelease suffix", result.stderr)
        self.assertEqual(output, "")


class CiWorkflowTests(unittest.TestCase):
    def test_the_signing_tests_run_against_the_pinned_minisign(self) -> None:
        steps = job_steps(workflow_jobs(CI.read_text())["check"])
        install = next(i for i, step in enumerate(steps) if "- name: Install minisign" in step)
        tests = next(i for i, step in enumerate(steps) if "- name: Release script tests" in step)
        self.assertLess(install, tests)
        self.assertIn('bash scripts/install-minisign.sh "$RUNNER_TEMP/minisign"', steps[install])
        self.assertIn("MINISIGN: ${{ steps.minisign.outputs.path }}", steps[tests])
        self.assertIn("if: matrix.os == 'ubuntu-latest'", steps[install])


if __name__ == "__main__":
    unittest.main()
