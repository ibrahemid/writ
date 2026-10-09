"""Tests for deploy_pages.py.

Run from the repository root:
    python3 -m unittest discover -s .github/scripts -t .github/scripts
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import pathlib
import signal
import sys
import tempfile
import unittest
from collections.abc import Callable
from unittest import mock

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import deploy_pages  # noqa: E402

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]

SHA = "e654d0e3" + "0" * 32
API = "https://api.github.com"
REPOSITORY = "ibrahemid/writ"
DEPLOYMENTS = f"{API}/repos/{REPOSITORY}/pages/deployments"
OIDC_URL = "https://token.actions.example/idtoken?api-version=2.0"
OIDC_TOKEN = "oidc.jwt.value"
PAGE_URL = "https://writ.example/"


def ok(payload: object, status: int = 200) -> deploy_pages.Response:
    return deploy_pages.Response(status, json.dumps(payload).encode(), "req-1")


def failed(status: int, message: str) -> deploy_pages.Response:
    return deploy_pages.Response(status, json.dumps({"message": message}).encode(), "req-2")


class FakeTransport:
    def __init__(self) -> None:
        self.routes: dict[tuple[str, str], list[deploy_pages.Response | Exception]] = {}
        self.calls: list[tuple[str, str, dict[str, str], bytes | None]] = []

    def add(self, method: str, url: str, *responses: deploy_pages.Response | Exception) -> None:
        self.routes.setdefault((method, url), []).extend(responses)

    def __call__(
        self, method: str, url: str, headers: dict[str, str], body: bytes | None
    ) -> deploy_pages.Response:
        self.calls.append((method, url, headers, body))
        queue = self.routes.get((method, url))
        if not queue:
            raise AssertionError(f"unexpected request {method} {url}")
        response = queue.pop(0) if len(queue) > 1 else queue[0]
        if isinstance(response, Exception):
            raise response
        return response

    def requests_to(self, method: str, url: str) -> list[tuple[str, str, dict[str, str], bytes | None]]:
        return [call for call in self.calls if call[0] == method and call[1] == url]


class FakeClock:
    def __init__(self, step: float = 5.0) -> None:
        self.now = 0.0
        self.step = step
        self.sleeps: list[float] = []

    def __call__(self) -> float:
        return self.now

    def sleep(self, seconds: float) -> None:
        self.sleeps.append(seconds)
        self.now += self.step


class BuildVersionTests(unittest.TestCase):
    def test_runs_of_one_commit_get_distinct_versions(self) -> None:
        push = deploy_pages.build_version(SHA, "36239400000", "1")
        release = deploy_pages.build_version(SHA, "36239469411", "1")
        rerun = deploy_pages.build_version(SHA, "36239469411", "2")

        self.assertEqual(len({push, release, rerun}), 3)
        self.assertEqual(release, f"{SHA}-36239469411-1")

    def test_refuses_values_that_are_not_a_commit_and_run_numbers(self) -> None:
        for sha, run_id, attempt in (
            ("e654d0e3", "1", "1"),
            (SHA, "", "1"),
            (SHA, "12a", "1"),
            (SHA, "1", "0x1"),
        ):
            with self.subTest(sha=sha, run_id=run_id, attempt=attempt):
                with self.assertRaises(deploy_pages.PagesDeployError):
                    deploy_pages.build_version(sha, run_id, attempt)


class OidcTokenTests(unittest.TestCase):
    def test_requests_the_token_with_the_runner_request_token(self) -> None:
        transport = FakeTransport()
        transport.add("GET", OIDC_URL, ok({"value": OIDC_TOKEN}))

        token = deploy_pages.fetch_oidc_token(transport, OIDC_URL, "request-token")

        self.assertEqual(token, OIDC_TOKEN)
        (_, _, headers, body), = transport.calls
        self.assertEqual(headers["Authorization"], "Bearer request-token")
        self.assertIsNone(body)

    def test_names_the_missing_permission_when_the_request_is_refused(self) -> None:
        transport = FakeTransport()
        transport.add("GET", OIDC_URL, failed(403, "forbidden"))

        with self.assertRaisesRegex(deploy_pages.PagesDeployError, "id-token: write"):
            deploy_pages.fetch_oidc_token(transport, OIDC_URL, "request-token")

    def test_refuses_a_response_without_a_token(self) -> None:
        transport = FakeTransport()
        transport.add("GET", OIDC_URL, ok({}))

        with self.assertRaises(deploy_pages.PagesDeployError):
            deploy_pages.fetch_oidc_token(transport, OIDC_URL, "request-token")


class DeploymentTests(unittest.TestCase):
    def setUp(self) -> None:
        self.transport = FakeTransport()
        self.clock = FakeClock()
        self.log = io.StringIO()
        api = deploy_pages.GitHubApi(self.transport, API, REPOSITORY, "gh-token")
        self.deployment = deploy_pages.PagesDeployment(
            api,
            clock=self.clock,
            sleep=self.clock.sleep,
            log=lambda line: self.log.write(line + "\n"),
        )
        self.version = deploy_pages.build_version(SHA, "36239469411", "1")

    def create(self, response: deploy_pages.Response | None = None) -> dict:
        self.transport.add(
            "POST",
            DEPLOYMENTS,
            response or ok({"id": "dep-7", "status_url": f"{DEPLOYMENTS}/dep-7", "page_url": PAGE_URL}),
        )
        return self.deployment.create(4242, self.version, OIDC_TOKEN)

    def statuses(self, *values: str | deploy_pages.Response | Exception) -> None:
        self.transport.add(
            "GET",
            f"{DEPLOYMENTS}/dep-7",
            *(ok({"status": value}) if isinstance(value, str) else value for value in values),
        )

    def cancels(self) -> int:
        return len(self.transport.requests_to("POST", f"{DEPLOYMENTS}/dep-7/cancel"))

    def test_create_sends_the_run_build_version_with_the_artifact_and_token(self) -> None:
        created = self.create()

        self.assertEqual(created["page_url"], PAGE_URL)
        (method, url, headers, body), = self.transport.calls
        self.assertEqual((method, url), ("POST", DEPLOYMENTS))
        self.assertEqual(
            json.loads(body or b""),
            {"artifact_id": 4242, "pages_build_version": self.version, "oidc_token": OIDC_TOKEN},
        )
        self.assertEqual(headers["Authorization"], "Bearer gh-token")
        self.assertEqual(headers["X-GitHub-Api-Version"], deploy_pages.API_VERSION)
        self.assertNotIn(OIDC_TOKEN, self.log.getvalue())

    def test_create_failure_says_what_to_check(self) -> None:
        for status, hint in ((403, "pages: write"), (404, "settings/pages"), (502, "outage")):
            with self.subTest(status=status):
                transport = FakeTransport()
                transport.add("POST", DEPLOYMENTS, failed(status, "nope"))
                api = deploy_pages.GitHubApi(transport, API, REPOSITORY, "gh-token")
                deployment = deploy_pages.PagesDeployment(api, log=lambda line: None)

                with self.assertRaisesRegex(deploy_pages.PagesDeployError, hint):
                    deployment.create(4242, self.version, OIDC_TOKEN)
                self.assertFalse(deployment.pending)

    def test_deployment_id_falls_back_to_the_status_url(self) -> None:
        self.create(ok({"status_url": f"{DEPLOYMENTS}/dep-7", "page_url": PAGE_URL}))
        self.statuses("succeed")

        self.deployment.wait()

        self.assertEqual(self.deployment.deployment_id, "dep-7")

    def test_wait_polls_until_the_deployment_succeeds(self) -> None:
        self.create()
        self.statuses("deployment_in_progress", "syncing_files", "succeed")

        self.deployment.wait()

        self.assertFalse(self.deployment.pending)
        self.assertEqual(self.clock.sleeps, [deploy_pages.POLL_INTERVAL_SECONDS] * 3)
        self.assertEqual(self.cancels(), 0)

    def test_a_final_error_status_fails_without_cancelling(self) -> None:
        self.create()
        self.statuses("syncing_files", "deployment_content_failed")

        with self.assertRaisesRegex(deploy_pages.PagesDeployError, "symlinks"):
            self.deployment.wait()
        self.deployment.cancel()

        self.assertEqual(self.cancels(), 0)

    def test_a_temporary_error_status_warns_and_keeps_polling(self) -> None:
        self.create()
        self.statuses("deployment_attempt_error", "succeed")

        self.deployment.wait()

        self.assertIn("::warning::", self.log.getvalue())

    def test_timing_out_leaves_the_deployment_for_cancel(self) -> None:
        self.create()
        self.statuses("deployment_in_progress")
        self.transport.add("POST", f"{DEPLOYMENTS}/dep-7/cancel", deploy_pages.Response(204, b"", ""))

        with self.assertRaisesRegex(deploy_pages.PagesDeployError, "Timed out"):
            self.deployment.wait()
        self.deployment.cancel()

        self.assertEqual(self.cancels(), 1)
        self.assertFalse(self.deployment.pending)
        self.assertGreaterEqual(self.clock.now, deploy_pages.TIMEOUT_SECONDS)

    def test_too_many_status_errors_leave_the_deployment_for_cancel(self) -> None:
        self.create()
        self.statuses(failed(500, "boom"))
        self.transport.add("POST", f"{DEPLOYMENTS}/dep-7/cancel", deploy_pages.Response(204, b"", ""))

        with self.assertRaisesRegex(deploy_pages.PagesDeployError, "500"):
            self.deployment.wait()
        self.deployment.cancel()

        status_reads = self.transport.requests_to("GET", f"{DEPLOYMENTS}/dep-7")
        self.assertEqual(len(status_reads), deploy_pages.MAX_ERRORS)
        self.assertEqual(self.cancels(), 1)

    def test_a_network_error_counts_as_a_status_error(self) -> None:
        self.create()
        self.statuses(deploy_pages.TransportError("connection reset"), "succeed")

        self.deployment.wait()

        self.assertIn("connection reset", self.log.getvalue())


class MainTests(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.output = pathlib.Path(tmp.name) / "github_output"
        self.output.write_text("")
        self.env = {
            "ARTIFACT_ID": "4242",
            "GITHUB_TOKEN": "gh-token",
            "GITHUB_REPOSITORY": REPOSITORY,
            "GITHUB_SHA": SHA,
            "GITHUB_RUN_ID": "36239469411",
            "GITHUB_RUN_ATTEMPT": "1",
            "GITHUB_API_URL": API,
            "ACTIONS_ID_TOKEN_REQUEST_URL": OIDC_URL,
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN": "request-token",
            "GITHUB_OUTPUT": str(self.output),
        }
        self.transport = FakeTransport()
        self.transport.add("GET", OIDC_URL, ok({"value": OIDC_TOKEN}))
        self.transport.add(
            "POST", DEPLOYMENTS, ok({"id": "dep-7", "status_url": f"{DEPLOYMENTS}/dep-7", "page_url": PAGE_URL})
        )
        self.transport.add("POST", f"{DEPLOYMENTS}/dep-7/cancel", deploy_pages.Response(204, b"", ""))
        self.clock = FakeClock()

    def run_main(self, sleep: Callable[[float], None] | None = None) -> tuple[int, str]:
        stdout = io.StringIO()
        with mock.patch.dict(os.environ, self.env, clear=True), contextlib.redirect_stdout(stdout):
            code = deploy_pages.main(self.transport, clock=self.clock, sleep=sleep or self.clock.sleep)
        return code, stdout.getvalue()

    def test_deploys_under_the_run_build_version_and_outputs_the_page_url(self) -> None:
        self.transport.add("GET", f"{DEPLOYMENTS}/dep-7", ok({"status": "succeed"}))

        code, stdout = self.run_main()

        self.assertEqual(code, 0)
        self.assertEqual(self.output.read_text(), f"page_url={PAGE_URL}\n")
        (_, _, _, body), = self.transport.requests_to("POST", DEPLOYMENTS)
        self.assertEqual(json.loads(body or b"")["pages_build_version"], f"{SHA}-36239469411-1")
        token_lines = [line for line in stdout.splitlines() if OIDC_TOKEN in line]
        self.assertEqual(token_lines, [f"::add-mask::{OIDC_TOKEN}"])
        self.assertLess(stdout.index("::add-mask::"), stdout.index("Created"))

    def test_job_cancellation_cancels_the_pending_deployment(self) -> None:
        self.transport.add("GET", f"{DEPLOYMENTS}/dep-7", ok({"status": "deployment_in_progress"}))
        previous = signal.getsignal(signal.SIGTERM)

        def cancelled_while_waiting(seconds: float) -> None:
            os.kill(os.getpid(), signal.SIGTERM)

        code, stdout = self.run_main(sleep=cancelled_while_waiting)

        self.assertEqual(code, 1)
        self.assertEqual(len(self.transport.requests_to("POST", f"{DEPLOYMENTS}/dep-7/cancel")), 1)
        self.assertIn("::error::", stdout)
        self.assertIs(signal.getsignal(signal.SIGTERM), previous)

    def test_a_failed_deployment_exits_non_zero(self) -> None:
        self.transport.add("GET", f"{DEPLOYMENTS}/dep-7", ok({"status": "deployment_failed"}))

        code, stdout = self.run_main()

        self.assertEqual(code, 1)
        self.assertIn("::error::Deployment failed", stdout)

    def test_missing_environment_fails_before_any_request(self) -> None:
        del self.env["ARTIFACT_ID"]
        del self.env["GITHUB_RUN_ATTEMPT"]

        code, stdout = self.run_main()

        self.assertEqual(code, 1)
        self.assertIn("ARTIFACT_ID", stdout)
        self.assertIn("GITHUB_RUN_ATTEMPT", stdout)
        self.assertEqual(self.transport.calls, [])

    def test_refuses_an_artifact_id_that_is_not_a_number(self) -> None:
        self.env["ARTIFACT_ID"] = "github-pages"

        code, _ = self.run_main()

        self.assertEqual(code, 1)
        self.assertEqual(self.transport.calls, [])


class SiteWorkflowTests(unittest.TestCase):
    def test_site_workflow_deploys_with_this_script_and_the_uploaded_artifact(self) -> None:
        workflow = (REPO_ROOT / ".github/workflows/site.yml").read_text()

        for absent in ("actions/deploy-pages@",):
            self.assertFalse(absent in workflow, f"site.yml still contains {absent}")
        for present in (
            "run: exec python3 -u .github/scripts/deploy_pages.py",
            "ARTIFACT_ID: ${{ needs.build.outputs.artifact_id }}",
            "artifact_id: ${{ steps.upload.outputs.artifact_id }}",
        ):
            self.assertTrue(present in workflow, f"site.yml lacks {present}")


if __name__ == "__main__":
    unittest.main()
