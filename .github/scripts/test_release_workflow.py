"""Structural checks on release.yml: which jobs see which secrets and tokens.

Run from the repository root:
    python3 -m unittest discover -s .github/scripts -t .github/scripts
"""

from __future__ import annotations

import os
import pathlib
import re
import subprocess
import tempfile
import unittest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
RELEASE = REPO_ROOT / ".github/workflows/release.yml"
CI = REPO_ROOT / ".github/workflows/ci.yml"

SIGNING_JOBS = {"signing-config", "sign-macos", "sign-windows", "sign-updater"}
PLAN_REF = "ref: refs/tags/${{ needs.plan.outputs.tag_name }}"
CACHE_ACTIONS = ("Swatinem/rust-cache@", "actions/cache@", "actions/cache/restore@", "actions/cache/save@")


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
            {"plan", "build", "sbom", "signing-config", "sign-macos", "sign-windows", "sign-updater", "attest-sboms", "publish"},
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

    def test_publish_checks_the_set_then_attests_every_asset(self) -> None:
        job = self.jobs["publish"]
        needs = job_field(job, "needs") or ""
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


class DispatchedTagTests(unittest.TestCase):
    """Runs plan's Resolve tag script, verbatim from release.yml, against a local origin."""

    @classmethod
    def setUpClass(cls) -> None:
        steps = job_steps(workflow_jobs(RELEASE.read_text())["plan"])
        [step] = [step for step in steps if step.startswith("      - name: Resolve tag")]
        cls.script = step_script(step)

    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory(prefix="writ-release-tag-test-")
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
        self.git("-C", str(work), "tag", "--annotate", "--message", "Writ 0.5.1", "v0.5.1")
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
