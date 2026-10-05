#!/usr/bin/env python3
"""Deploy this run's Pages artifact under a build version unique to the run.

actions/deploy-pages sends GITHUB_SHA as pages_build_version, and a runner
step cannot override GITHUB_SHA. Pages treats a deployment whose build version
it already has as that earlier deployment, so a run that redeploys an already
deployed commit (a release published on the commit the last push deployed)
leaves the old site live. This script makes the same REST calls as
deploy-pages with the version <sha>-<run id>-<run attempt>.

Expected environment variables:
  ARTIFACT_ID                     id of the github-pages artifact this run uploaded
  GITHUB_TOKEN                    token with pages: write
  GITHUB_REPOSITORY, GITHUB_SHA, GITHUB_RUN_ID, GITHUB_RUN_ATTEMPT, GITHUB_API_URL
  ACTIONS_ID_TOKEN_REQUEST_URL,
  ACTIONS_ID_TOKEN_REQUEST_TOKEN  set by the runner when the job has id-token: write
  GITHUB_OUTPUT                   receives page_url

Optional: GITHUB_SERVER_URL (default https://github.com), for error hints.
"""
from __future__ import annotations

import dataclasses
import json
import os
import re
import signal
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Callable
from typing import Any

API_VERSION = "2022-11-28"
USER_AGENT = "writ-pages-deploy"
REQUEST_TIMEOUT_SECONDS = 30.0
POLL_INTERVAL_SECONDS = 5.0
TIMEOUT_SECONDS = 600.0
MAX_ERRORS = 10
MAX_ERROR_BACKOFF_SECONDS = 15.0

REQUIRED_ENV = (
    "ARTIFACT_ID",
    "GITHUB_TOKEN",
    "GITHUB_REPOSITORY",
    "GITHUB_SHA",
    "GITHUB_RUN_ID",
    "GITHUB_RUN_ATTEMPT",
    "GITHUB_API_URL",
    "ACTIONS_ID_TOKEN_REQUEST_URL",
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
    "GITHUB_OUTPUT",
)

TEMPORARY_STATUSES = {
    "unknown_status": "Unable to get deployment status.",
    "not_found": "Deployment not found.",
    "deployment_attempt_error": "Deployment temporarily failed, a retry will be automatically scheduled...",
}

FINAL_ERROR_STATUSES = {
    "deployment_failed": "Deployment failed, try again later.",
    "deployment_content_failed": (
        "Artifact could not be deployed. Please ensure the content does not contain any hard links, "
        "symlinks and total size is less than 10GB."
    ),
    "deployment_cancelled": "Deployment cancelled.",
    "deployment_lost": "Deployment failed to report final status.",
}


class PagesDeployError(RuntimeError):
    pass


class TransportError(PagesDeployError):
    pass


class ApiError(PagesDeployError):
    def __init__(self, status: int, message: str, request_id: str) -> None:
        detail = f"HTTP {status}: {message}" if message else f"HTTP {status}"
        if request_id:
            detail += f" (request {request_id})"
        super().__init__(detail)
        self.status = status


class DeploymentInterrupted(Exception):
    pass


@dataclasses.dataclass(frozen=True)
class Response:
    status: int
    body: bytes
    request_id: str


Transport = Callable[[str, str, dict[str, str], bytes | None], Response]


def urllib_transport(method: str, url: str, headers: dict[str, str], body: bytes | None) -> Response:
    request = urllib.request.Request(url, data=body, headers=headers, method=method)
    try:
        with urllib.request.urlopen(request, timeout=REQUEST_TIMEOUT_SECONDS) as response:
            return Response(response.status, response.read(), response.headers.get("x-github-request-id", ""))
    except urllib.error.HTTPError as error:
        request_id = error.headers.get("x-github-request-id", "") if error.headers else ""
        return Response(error.code, error.read(), request_id)
    except (urllib.error.URLError, OSError) as error:
        raise TransportError(f"{method} {url} failed: {error}") from error


def error_message(response: Response) -> str:
    try:
        payload = json.loads(response.body)
    except ValueError:
        return response.body.decode("utf-8", errors="replace").strip()[:200]
    if isinstance(payload, dict) and isinstance(payload.get("message"), str):
        return payload["message"]
    return ""


def build_version(sha: str, run_id: str, run_attempt: str) -> str:
    if not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", sha):
        raise PagesDeployError(f"GITHUB_SHA is not a full commit id: {sha!r}")
    for name, value in (("GITHUB_RUN_ID", run_id), ("GITHUB_RUN_ATTEMPT", run_attempt)):
        if not re.fullmatch(r"[0-9]+", value):
            raise PagesDeployError(f"{name} is not a number: {value!r}")
    return f"{sha}-{run_id}-{run_attempt}"


def fetch_oidc_token(transport: Transport, request_url: str, request_token: str) -> str:
    response = transport(
        "GET",
        request_url,
        {
            "Authorization": f"Bearer {request_token}",
            "Accept": "application/json",
            "User-Agent": USER_AGENT,
        },
        None,
    )
    if response.status != 200:
        raise PagesDeployError(
            f"OIDC token request failed with HTTP {response.status}; "
            'the deploy job needs the permission "id-token: write"'
        )
    try:
        value = json.loads(response.body).get("value")
    except (ValueError, AttributeError) as error:
        raise PagesDeployError("OIDC token response is not a JSON object") from error
    if not isinstance(value, str) or not value:
        raise PagesDeployError("OIDC token response carries no token")
    return value


class GitHubApi:
    def __init__(
        self,
        transport: Transport,
        api_url: str,
        repository: str,
        token: str,
        server_url: str = "https://github.com",
    ) -> None:
        self.transport = transport
        self.api_url = api_url.rstrip("/")
        self.repository = repository
        self.token = token
        self.server_url = server_url.rstrip("/")

    def request(self, method: str, path: str, payload: dict[str, Any] | None = None) -> dict[str, Any]:
        headers = {
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {self.token}",
            "X-GitHub-Api-Version": API_VERSION,
            "User-Agent": USER_AGENT,
        }
        body = None
        if payload is not None:
            headers["Content-Type"] = "application/json"
            body = json.dumps(payload).encode()
        response = self.transport(method, f"{self.api_url}/repos/{self.repository}{path}", headers, body)
        if not 200 <= response.status < 300:
            raise ApiError(response.status, error_message(response), response.request_id)
        if not response.body:
            return {}
        try:
            data = json.loads(response.body)
        except ValueError as error:
            raise PagesDeployError(f"{method} {path} returned a body that is not JSON") from error
        if not isinstance(data, dict):
            raise PagesDeployError(f"{method} {path} returned JSON that is not an object")
        return data


class PagesDeployment:
    def __init__(
        self,
        api: GitHubApi,
        *,
        clock: Callable[[], float] = time.monotonic,
        sleep: Callable[[float], None] = time.sleep,
        log: Callable[[str], None] = print,
    ) -> None:
        self.api = api
        self.clock = clock
        self.sleep = sleep
        self.log = log
        self.deployment_id: str | None = None
        self.pending = False
        self.started = 0.0

    def create(self, artifact_id: int, version: str, oidc_token: str) -> dict[str, Any]:
        try:
            created = self.api.request(
                "POST",
                "/pages/deployments",
                {"artifact_id": artifact_id, "pages_build_version": version, "oidc_token": oidc_token},
            )
        except ApiError as error:
            raise PagesDeployError(self.create_failure(error, version)) from error
        status_url = created.get("status_url") or ""
        deployment_id = created.get("id") or status_url.rstrip("/").rsplit("/", 1)[-1] or version
        self.deployment_id = str(deployment_id)
        self.pending = True
        self.started = self.clock()
        self.log(
            f"Created Pages deployment {self.deployment_id} for artifact {artifact_id}, "
            f"build version {version}"
        )
        return created

    def create_failure(self, error: ApiError, version: str) -> str:
        message = f"Creating the Pages deployment for build version {version} failed: {error}."
        if error.status == 403:
            message += ' The token needs the permission "pages: write".'
        elif error.status == 404:
            message += f" Check that Pages is enabled: {self.api.server_url}/{self.api.repository}/settings/pages"
        elif error.status >= 500:
            message += " Check githubstatus.com for a Pages outage and re-run the deployment later."
        return message

    def status_path(self) -> str:
        return f"/pages/deployments/{urllib.parse.quote(self.deployment_id or '', safe='')}"

    def wait(self) -> None:
        errors = 0
        backoff = 0.0
        last_error: PagesDeployError | None = None
        while True:
            self.sleep(POLL_INTERVAL_SECONDS + backoff)
            try:
                status = self.api.request("GET", self.status_path()).get("status", "")
            except PagesDeployError as error:
                errors += 1
                last_error = error
                backoff = min(MAX_ERROR_BACKOFF_SECONDS, backoff * 2 + 1)
                self.log(f"::warning::Reading the deployment status failed: {error}")
            else:
                backoff = 0.0
                if status == "succeed":
                    self.pending = False
                    self.log("Reported success!")
                    return
                if status in FINAL_ERROR_STATUSES:
                    self.pending = False
                    raise PagesDeployError(FINAL_ERROR_STATUSES[status])
                if status in TEMPORARY_STATUSES:
                    self.log(f"::warning::{TEMPORARY_STATUSES[status]}")
                else:
                    self.log(f"Current status: {status}")
            if errors >= MAX_ERRORS:
                raise PagesDeployError(
                    f"Too many errors reading the deployment status, last: {last_error}"
                )
            if self.clock() - self.started >= TIMEOUT_SECONDS:
                raise PagesDeployError(
                    f"Timed out after {int(TIMEOUT_SECONDS)} seconds waiting for the Pages deployment"
                )

    def cancel(self) -> None:
        if not self.pending or self.deployment_id is None:
            return
        try:
            self.api.request("POST", f"{self.status_path()}/cancel")
        except PagesDeployError as error:
            self.log(f"::error::Cancelling Pages deployment {self.deployment_id} failed: {error}")
            return
        self.pending = False
        self.log(f"Cancelled Pages deployment {self.deployment_id}")


def read_environment() -> dict[str, str]:
    missing = [name for name in REQUIRED_ENV if not os.environ.get(name)]
    if missing:
        raise PagesDeployError(f"Missing required env var: {', '.join(missing)}")
    return {name: os.environ[name] for name in REQUIRED_ENV}


def parse_artifact_id(value: str) -> int:
    if not re.fullmatch(r"[0-9]+", value):
        raise PagesDeployError(f"ARTIFACT_ID is not a number: {value!r}")
    return int(value)


def write_output(path: str, name: str, value: str) -> None:
    if "\n" in value or "\r" in value:
        raise PagesDeployError(f"output {name} contains a line break")
    with open(path, "a", encoding="utf-8") as output:
        output.write(f"{name}={value}\n")


def raise_interrupted(signum: int, _frame: object) -> None:
    raise DeploymentInterrupted(f"Interrupted by {signal.Signals(signum).name}")


def main(
    transport: Transport = urllib_transport,
    *,
    clock: Callable[[], float] = time.monotonic,
    sleep: Callable[[float], None] = time.sleep,
) -> int:
    try:
        env = read_environment()
        version = build_version(env["GITHUB_SHA"], env["GITHUB_RUN_ID"], env["GITHUB_RUN_ATTEMPT"])
        artifact_id = parse_artifact_id(env["ARTIFACT_ID"])
        oidc_token = fetch_oidc_token(
            transport, env["ACTIONS_ID_TOKEN_REQUEST_URL"], env["ACTIONS_ID_TOKEN_REQUEST_TOKEN"]
        )
    except PagesDeployError as error:
        print(f"::error::{error}")
        return 1
    print(f"::add-mask::{oidc_token}", flush=True)

    api = GitHubApi(
        transport,
        env["GITHUB_API_URL"],
        env["GITHUB_REPOSITORY"],
        env["GITHUB_TOKEN"],
        os.environ.get("GITHUB_SERVER_URL") or "https://github.com",
    )
    deployment = PagesDeployment(api, clock=clock, sleep=sleep)
    previous_handlers = {
        signum: signal.signal(signum, raise_interrupted) for signum in (signal.SIGINT, signal.SIGTERM)
    }
    try:
        created = deployment.create(artifact_id, version, oidc_token)
        write_output(env["GITHUB_OUTPUT"], "page_url", str(created.get("page_url") or ""))
        deployment.wait()
    except (PagesDeployError, DeploymentInterrupted) as error:
        print(f"::error::{error}")
        deployment.cancel()
        return 1
    finally:
        for signum, handler in previous_handlers.items():
            signal.signal(signum, handler)
    return 0


if __name__ == "__main__":
    sys.exit(main())
