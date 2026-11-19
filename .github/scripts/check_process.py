"""Start processes for the upgrade checks with a deadline and stop their whole tree.

Output goes to files, not pipes: a GUI child that inherits a pipe keeps it open
after its parent exits, and reading that pipe to its end then never returns.
"""

from __future__ import annotations

import dataclasses
import io
import os
import pathlib
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import unittest
from collections.abc import Mapping, Sequence
from typing import TextIO

STOP_GRACE = 15.0
TASKKILL_TIMEOUT = 60.0
OUTPUT_TAIL = 4000


class ProcessDeadlineError(RuntimeError):
    pass


@dataclasses.dataclass
class StartedProcess:
    command: str | tuple[str, ...]
    process: subprocess.Popen[bytes]
    folder: pathlib.Path

    def read_output(self) -> tuple[str, str]:
        return read_text(self.folder / "stdout"), read_text(self.folder / "stderr")

    def stop(self, grace: float = STOP_GRACE) -> tuple[str, str]:
        try:
            stop_process_tree(self.process, grace)
            return self.read_output()
        finally:
            shutil.rmtree(self.folder, ignore_errors=True)


def describe_command(command: str | Sequence[str]) -> str:
    if isinstance(command, str):
        return command
    return subprocess.list2cmdline(command) if sys.platform == "win32" else shlex.join(command)


def read_text(path: pathlib.Path) -> str:
    return path.read_bytes().decode("utf-8", errors="replace") if path.is_file() else ""


def get_tail(text: str, limit: int = OUTPUT_TAIL) -> str:
    return text if len(text) <= limit else text[-limit:]


def prepare_output_streams(*streams: TextIO) -> None:
    for stream in streams or (sys.stdout, sys.stderr):
        if isinstance(stream, io.TextIOWrapper):
            stream.reconfigure(encoding="utf-8", errors="backslashreplace")


def get_group_options() -> dict[str, bool]:
    return {} if sys.platform == "win32" else {"start_new_session": True}


def start_process(
    args: str | Sequence[str],
    *,
    env: Mapping[str, str] | None = None,
    cwd: str | os.PathLike[str] | None = None,
    merge_output: bool = False,
) -> StartedProcess:
    command: str | tuple[str, ...] = args if isinstance(args, str) else tuple(str(arg) for arg in args)
    folder = pathlib.Path(tempfile.mkdtemp(prefix="check-process-"))
    try:
        with open(folder / "stdout", "wb") as stdout, open(folder / "stderr", "wb") as stderr:
            process = subprocess.Popen(
                command if isinstance(command, str) else list(command),
                stdin=subprocess.DEVNULL,
                stdout=stdout,
                stderr=subprocess.STDOUT if merge_output else stderr,
                env=None if env is None else dict(env),
                cwd=cwd,
                **get_group_options(),
            )
    except BaseException:
        shutil.rmtree(folder, ignore_errors=True)
        raise
    return StartedProcess(command, process, folder)


def signal_group(pid: int, sent: signal.Signals) -> None:
    try:
        os.killpg(pid, sent)
    except (ProcessLookupError, PermissionError):
        pass


def stop_process_tree(process: subprocess.Popen, grace: float = STOP_GRACE) -> None:
    if sys.platform == "win32":
        if process.poll() is None:
            subprocess.run(
                ["taskkill", "/F", "/T", "/PID", str(process.pid)],
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=TASKKILL_TIMEOUT,
                check=False,
            )
    else:
        if process.poll() is None:
            signal_group(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=grace)
            except subprocess.TimeoutExpired:
                pass
        signal_group(process.pid, signal.SIGKILL)
    try:
        process.wait(timeout=grace)
    except subprocess.TimeoutExpired as error:
        raise ProcessDeadlineError(
            f"{describe_command(process.args)} (pid {process.pid}) still runs {grace:.0f}s after its process tree was killed"
        ) from error


def run_bounded(
    args: str | Sequence[str],
    *,
    timeout: float,
    env: Mapping[str, str] | None = None,
    cwd: str | os.PathLike[str] | None = None,
) -> subprocess.CompletedProcess[str]:
    started = start_process(args, env=env, cwd=cwd)
    try:
        try:
            returncode = started.process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            stop_process_tree(started.process)
            stdout, stderr = started.read_output()
            raise ProcessDeadlineError(
                f"{describe_command(started.command)} was still running after {timeout:.0f}s, so its process tree was "
                f"killed\nstdout tail:\n{get_tail(stdout)}\nstderr tail:\n{get_tail(stderr)}"
            ) from None
        stdout, stderr = started.read_output()
        command = started.command if isinstance(started.command, str) else list(started.command)
        return subprocess.CompletedProcess(command, returncode, stdout, stderr)
    finally:
        try:
            if started.process.poll() is None:
                stop_process_tree(started.process)
            elif sys.platform != "win32":
                signal_group(started.process.pid, signal.SIGKILL)
        finally:
            shutil.rmtree(started.folder, ignore_errors=True)


class ImmediateTextResult(unittest.TextTestResult):
    def addFailure(self, test: unittest.TestCase, err) -> None:
        super().addFailure(test, err)
        self.print_now("FAIL", test, self.failures[-1][1])

    def addError(self, test: unittest.TestCase, err) -> None:
        super().addError(test, err)
        self.print_now("ERROR", test, self.errors[-1][1])

    def addSubTest(self, test: unittest.TestCase, subtest: unittest.TestCase, err) -> None:
        super().addSubTest(test, subtest, err)
        if err is not None:
            failed = issubclass(err[0], test.failureException)
            self.print_now("FAIL" if failed else "ERROR", subtest, (self.failures if failed else self.errors)[-1][1])

    def print_now(self, kind: str, test: unittest.TestCase, detail: str) -> None:
        self.stream.writeln(f"{kind}: {self.getDescription(test)}")
        self.stream.writeln(detail)
        self.stream.flush()
