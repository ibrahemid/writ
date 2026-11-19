from __future__ import annotations

import io
import os
import pathlib
import sys
import tempfile
import time
import unittest

from check_process import ImmediateTextResult, ProcessDeadlineError, prepare_output_streams, run_bounded, start_process

POSIX_ONLY = unittest.skipIf(sys.platform == "win32", "process groups are POSIX")


def is_process_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def wait_for_exit(pid: int, timeout: float) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if not is_process_alive(pid):
            return True
        time.sleep(0.05)
    return not is_process_alive(pid)


def make_folder(test: unittest.TestCase) -> pathlib.Path:
    folder = tempfile.TemporaryDirectory()
    test.addCleanup(folder.cleanup)
    return pathlib.Path(folder.name)


def read_pid(path: pathlib.Path, timeout: float) -> int:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        text = path.read_text().strip() if path.is_file() else ""
        if text.isdigit():
            return int(text)
        time.sleep(0.05)
    raise AssertionError(f"no pid was written to {path} within {timeout}s")


@POSIX_ONLY
class RunBoundedTests(unittest.TestCase):
    def test_it_returns_the_exit_code_and_both_outputs(self) -> None:
        result = run_bounded(["sh", "-c", "echo out; echo err >&2; exit 3"], timeout=10)
        self.assertEqual((result.returncode, result.stdout, result.stderr), (3, "out\n", "err\n"))

    def test_it_passes_the_environment_and_folder(self) -> None:
        folder = make_folder(self)
        result = run_bounded(["sh", "-c", 'echo "$CHECK_VALUE $(pwd -P)"'], timeout=10, env={**os.environ, "CHECK_VALUE": "seen"}, cwd=folder)
        self.assertEqual(result.stdout.strip(), f"seen {folder.resolve()}")

    def test_an_overrun_is_killed_and_reported_with_its_output(self) -> None:
        started = time.monotonic()
        with self.assertRaises(ProcessDeadlineError) as raised:
            run_bounded(["sh", "-c", "echo before-the-wait; sleep 30"], timeout=1)
        self.assertLess(time.monotonic() - started, 20)
        message = str(raised.exception)
        self.assertIn("was still running after 1s", message)
        self.assertIn("before-the-wait", message)

    def test_a_child_that_keeps_the_output_open_does_not_hold_the_caller(self) -> None:
        folder = make_folder(self)
        marker = folder / "child.pid"
        started = time.monotonic()
        result = run_bounded(["sh", "-c", f'sleep 30 & echo $! > "{marker}"; echo started'], timeout=10)
        self.assertLess(time.monotonic() - started, 10)
        self.assertEqual(result.stdout, "started\n")
        self.assertTrue(wait_for_exit(read_pid(marker, 5), 10), "the background child outlived run_bounded")


@POSIX_ONLY
class StopProcessTreeTests(unittest.TestCase):
    def test_stopping_kills_the_child_processes_too(self) -> None:
        folder = make_folder(self)
        marker = folder / "child.pid"
        started = start_process(["sh", "-c", f'sleep 60 & echo $! > "{marker}"; wait'])
        try:
            child = read_pid(marker, 5)
            self.assertTrue(is_process_alive(child))
        finally:
            started.stop(grace=5)
        self.assertIsNotNone(started.process.poll())
        self.assertTrue(wait_for_exit(child, 10), "the child process survived the stop")

    def test_a_process_that_ignores_the_polite_signal_is_killed(self) -> None:
        started = start_process(["sh", "-c", "trap '' TERM; echo ready; while :; do sleep 1; done"])
        try:
            deadline = time.monotonic() + 5
            while "ready" not in started.read_output()[0] and time.monotonic() < deadline:
                time.sleep(0.05)
            began = time.monotonic()
        finally:
            stdout, _ = started.stop(grace=1)
        self.assertLess(time.monotonic() - began, 10)
        self.assertEqual(stdout, "ready\n")
        self.assertIsNotNone(started.process.poll())

    def test_merged_output_lands_in_stdout(self) -> None:
        started = start_process(["sh", "-c", "echo one; echo two >&2"], merge_output=True)
        started.process.wait(timeout=10)
        stdout, stderr = started.stop()
        self.assertEqual((stdout, stderr), ("one\ntwo\n", ""))


class ImmediateTextResultTests(unittest.TestCase):
    def test_a_failure_is_printed_before_the_next_check_runs(self) -> None:
        stream = io.StringIO()
        seen: list[str] = []

        class Checks(unittest.TestCase):
            def test_1_fails(self) -> None:
                self.fail("the registry names another launcher")

            def test_2_reads_the_stream(self) -> None:
                seen.append(stream.getvalue())

        runner = unittest.TextTestRunner(stream=stream, resultclass=ImmediateTextResult, verbosity=0)
        result = runner.run(unittest.defaultTestLoader.loadTestsFromTestCase(Checks))
        self.assertEqual(len(result.failures), 1)
        self.assertIn("FAIL: test_1_fails", seen[0])
        self.assertIn("the registry names another launcher", seen[0])

    def test_a_failed_subtest_is_printed_with_its_parameters(self) -> None:
        stream = io.StringIO()

        class Checks(unittest.TestCase):
            def test_protocols(self) -> None:
                for name in ("jotra", "writ"):
                    with self.subTest(protocol=name):
                        self.assertEqual(name, "jotra")

        runner = unittest.TextTestRunner(stream=stream, resultclass=ImmediateTextResult, verbosity=0)
        runner.run(unittest.defaultTestLoader.loadTestsFromTestCase(Checks))
        self.assertIn("protocol='writ'", stream.getvalue())
        self.assertNotIn("protocol='jotra'", stream.getvalue())


class PrepareOutputStreamsTests(unittest.TestCase):
    def test_a_legacy_code_page_stream_writes_utf8(self) -> None:
        raw = io.BytesIO()
        stream = io.TextIOWrapper(raw, encoding="cp1252")
        with self.assertRaises(UnicodeEncodeError):
            stream.write("█ progress\n")
            stream.flush()
        prepare_output_streams(stream)
        stream.write("█ progress\n")
        stream.flush()
        self.assertEqual(raw.getvalue(), "█ progress\n".encode())

    def test_an_unencodable_character_is_escaped(self) -> None:
        raw = io.BytesIO()
        stream = io.TextIOWrapper(raw, encoding="cp1252")
        prepare_output_streams(stream)
        stream.write("lone \udc80 surrogate\n")
        stream.flush()
        self.assertEqual(raw.getvalue(), b"lone \\udc80 surrogate\n")

    def test_a_stream_without_an_encoding_is_left_alone(self) -> None:
        stream = io.StringIO()
        prepare_output_streams(stream)
        stream.write("█")
        self.assertEqual(stream.getvalue(), "█")


if __name__ == "__main__":
    unittest.main()
