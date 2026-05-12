import { describe, it, expect } from "vitest";
import {
  ShortcutRecorder,
  findConflicts,
  DOUBLE_TAP_WINDOW_MS,
} from "../../../components/ShortcutEditor/recorder";

describe("ShortcutRecorder", () => {
  it("emits a double-tap chord when bare Shift fires twice within the window", () => {
    const recorder = new ShortcutRecorder();
    expect(recorder.handle({ key: "Shift", shiftKey: true }, 0)).toEqual({
      kind: "waiting",
    });
    expect(
      recorder.handle({ key: "Shift", shiftKey: true }, DOUBLE_TAP_WINDOW_MS - 1),
    ).toEqual({ kind: "captured", binding: "Shift+Shift" });
  });

  it("does not chord when the second bare Shift lands after the window", () => {
    const recorder = new ShortcutRecorder();
    recorder.handle({ key: "Shift", shiftKey: true }, 0);
    expect(
      recorder.handle({ key: "Shift", shiftKey: true }, DOUBLE_TAP_WINDOW_MS + 5),
    ).toEqual({ kind: "waiting" });
  });

  it("captures CmdOrCtrl+Shift+K as a normalized chord", () => {
    const recorder = new ShortcutRecorder();
    const outcome = recorder.handle({
      key: "k",
      metaKey: true,
      shiftKey: true,
    });
    expect(outcome).toEqual({ kind: "captured", binding: "CmdOrCtrl+Shift+K" });
  });

  it("uppercases single character keys", () => {
    const recorder = new ShortcutRecorder();
    const outcome = recorder.handle({ key: "n", metaKey: true });
    expect(outcome).toEqual({ kind: "captured", binding: "CmdOrCtrl+N" });
  });

  it("translates Space into the Space token", () => {
    const recorder = new ShortcutRecorder();
    const outcome = recorder.handle({ key: " ", metaKey: true });
    expect(outcome).toEqual({ kind: "captured", binding: "CmdOrCtrl+Space" });
  });

  it("cancels and clears pending state on Escape", () => {
    const recorder = new ShortcutRecorder();
    recorder.handle({ key: "Shift", shiftKey: true });
    expect(recorder.handle({ key: "Escape" })).toEqual({ kind: "cancelled" });
    expect(
      recorder.handle({ key: "Shift", shiftKey: true }, DOUBLE_TAP_WINDOW_MS),
    ).toEqual({ kind: "waiting" });
  });

  it("ignores a bare modifier followed by a different bare modifier", () => {
    const recorder = new ShortcutRecorder();
    recorder.handle({ key: "Shift", shiftKey: true }, 0);
    expect(recorder.handle({ key: "Control", ctrlKey: true }, 50)).toEqual({
      kind: "waiting",
    });
  });

  it("reset clears pending tap so the next press starts fresh", () => {
    const recorder = new ShortcutRecorder();
    recorder.handle({ key: "Shift", shiftKey: true }, 0);
    recorder.reset();
    expect(recorder.handle({ key: "Shift", shiftKey: true }, 10)).toEqual({
      kind: "waiting",
    });
  });
});

describe("findConflicts", () => {
  it("flags two drafts that share the same normalized binding", () => {
    const conflicts = findConflicts({
      "a.cmd": "CmdOrCtrl+S",
      "b.cmd": "CmdOrCtrl+S",
      "c.cmd": "CmdOrCtrl+T",
    });
    expect(conflicts.get("a.cmd")).toEqual(["b.cmd"]);
    expect(conflicts.get("b.cmd")).toEqual(["a.cmd"]);
    expect(conflicts.has("c.cmd")).toBe(false);
  });

  it("ignores empty bindings", () => {
    const conflicts = findConflicts({ "a.cmd": "", "b.cmd": "" });
    expect(conflicts.size).toBe(0);
  });
});
