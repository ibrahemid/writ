import { describe, it, expect } from "vitest";
import {
  ShortcutRecorder,
  findConflicts,
  findFixedConflicts,
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

  it("records Command as CmdOrCtrl and Control as Ctrl on macOS", () => {
    const recorder = new ShortcutRecorder(DOUBLE_TAP_WINDOW_MS, "mac");
    expect(recorder.handle({ key: "k", code: "KeyK", metaKey: true, shiftKey: true })).toEqual({
      kind: "captured",
      binding: "CmdOrCtrl+Shift+K",
    });
    expect(recorder.handle({ key: "n", code: "KeyN", ctrlKey: true })).toEqual({
      kind: "captured",
      binding: "Ctrl+N",
    });
  });

  it("records Control as CmdOrCtrl on Windows and Linux", () => {
    for (const platform of ["win", "linux"] as const) {
      const recorder = new ShortcutRecorder(DOUBLE_TAP_WINDOW_MS, platform);
      expect(recorder.handle({ key: "n", code: "KeyN", ctrlKey: true })).toEqual({
        kind: "captured",
        binding: "CmdOrCtrl+N",
      });
    }
  });

  it("records the key's position on a non-Latin layout", () => {
    const recorder = new ShortcutRecorder(DOUBLE_TAP_WINDOW_MS, "win");
    expect(recorder.handle({ key: "ى", code: "KeyN", ctrlKey: true })).toEqual({
      kind: "captured",
      binding: "CmdOrCtrl+N",
    });
  });

  it("records punctuation and Space by the character", () => {
    const recorder = new ShortcutRecorder(DOUBLE_TAP_WINDOW_MS, "mac");
    expect(recorder.handle({ key: "\\", code: "Backslash", metaKey: true })).toEqual({
      kind: "captured",
      binding: "CmdOrCtrl+\\",
    });
    expect(recorder.handle({ key: " ", code: "Space", metaKey: true })).toEqual({
      kind: "captured",
      binding: "CmdOrCtrl+Space",
    });
  });

  it("records a function key on its own, and names any other bare key without capturing it", () => {
    const recorder = new ShortcutRecorder(DOUBLE_TAP_WINDOW_MS, "mac");
    expect(recorder.handle({ key: "F2", code: "F2" })).toEqual({ kind: "captured", binding: "F2" });
    expect(recorder.handle({ key: "a", code: "KeyA" })).toEqual({ kind: "bare", binding: "A" });
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

describe("findConflicts across platforms", () => {
  it("counts a Control chord and a Command chord as one key off macOS", () => {
    const draft = { "a.cmd": "Ctrl+K", "b.cmd": "CmdOrCtrl+K" };
    expect(findConflicts(draft, "win").get("a.cmd")).toEqual(["b.cmd"]);
    expect(findConflicts(draft, "mac").size).toBe(0);
  });
});

describe("findFixedConflicts", () => {
  const fixed = [{ chord: "CmdOrCtrl+Q", label: "Quit Writ" }];

  it("names the menu item a draft would collide with", () => {
    const conflicts = findFixedConflicts(
      { "a.cmd": "CmdOrCtrl+Q", "b.cmd": "CmdOrCtrl+T" },
      fixed,
      "mac",
    );
    expect(conflicts.get("a.cmd")).toBe("Quit Writ");
    expect(conflicts.has("b.cmd")).toBe(false);
  });

  it("finds nothing with no fixed chords", () => {
    expect(findFixedConflicts({ "a.cmd": "CmdOrCtrl+Q" }, [], "win").size).toBe(0);
  });
});
