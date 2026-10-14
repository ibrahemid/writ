import { describe, it, expect } from "vitest";
import {
  canonicalChord,
  chordForRecording,
  sameChord,
  chordFromKey,
  chordFromPosition,
  chordParts,
  worksFromAnyApp,
} from "../../lib/chord";

const altGraph = (on: boolean) => (modifier: string) => modifier === "AltGraph" && on;

describe("chordFromKey", () => {
  it("reads Command as CmdOrCtrl and Control as Ctrl on macOS", () => {
    expect(chordFromKey({ key: "n", metaKey: true }, "mac")).toBe("CmdOrCtrl+N");
    expect(chordFromKey({ key: "n", ctrlKey: true }, "mac")).toBe("Ctrl+N");
    expect(chordFromKey({ key: "n", metaKey: true, ctrlKey: true }, "mac")).toBe(
      "CmdOrCtrl+Ctrl+N",
    );
  });

  it("reads Control as CmdOrCtrl on Windows and Linux", () => {
    expect(chordFromKey({ key: "n", ctrlKey: true }, "win")).toBe("CmdOrCtrl+N");
    expect(chordFromKey({ key: "n", ctrlKey: true }, "linux")).toBe("CmdOrCtrl+N");
  });

  it("spells Space and uppercases a character", () => {
    expect(chordFromKey({ key: " ", metaKey: true }, "mac")).toBe("CmdOrCtrl+Space");
    expect(chordFromKey({ key: "k", metaKey: true, shiftKey: true }, "mac")).toBe(
      "CmdOrCtrl+Shift+K",
    );
  });
});

describe("chordFromPosition", () => {
  it("reads the Latin letter at the key's position on an Arabic layout", () => {
    expect(chordFromPosition({ key: "ى", code: "KeyN", ctrlKey: true }, "win")).toBe(
      "CmdOrCtrl+N",
    );
  });

  it("does the same for Russian and Greek letters", () => {
    expect(chordFromPosition({ key: "т", code: "KeyN", ctrlKey: true }, "linux")).toBe(
      "CmdOrCtrl+N",
    );
    expect(
      chordFromPosition({ key: "Φ", code: "KeyF", ctrlKey: true, shiftKey: true }, "win"),
    ).toBe("CmdOrCtrl+Shift+F");
  });

  it("reads the letter under an Option-composed character on macOS", () => {
    expect(
      chordFromPosition({ key: "ƒ", code: "KeyF", metaKey: true, altKey: true }, "mac"),
    ).toBe("CmdOrCtrl+Alt+F");
  });

  it("reads the digit under a shifted digit key", () => {
    expect(
      chordFromPosition({ key: ")", code: "Digit0", metaKey: true, shiftKey: true }, "mac"),
    ).toBe("CmdOrCtrl+Shift+0");
  });

  // Shift+\ types "|", so ⌘⇧\ arrived as a chord nothing is bound to and the
  // connections chord answered only through the macOS menu bar.
  it("reads the punctuation key under a shifted punctuation key", () => {
    expect(
      chordFromPosition({ key: "|", code: "Backslash", metaKey: true, shiftKey: true }, "mac"),
    ).toBe("CmdOrCtrl+Shift+\\");
    expect(
      chordFromPosition({ key: "?", code: "Slash", ctrlKey: true, shiftKey: true }, "win"),
    ).toBe("CmdOrCtrl+Shift+/");
  });

  it("adds nothing for a Latin letter, so AZERTY and Dvorak keep the printed letter", () => {
    expect(chordFromPosition({ key: "a", code: "KeyQ", metaKey: true }, "mac")).toBeNull();
  });

  it("adds nothing under AltGr, where Ctrl+Alt types a character", () => {
    expect(
      chordFromPosition(
        { key: "ś", code: "KeyS", ctrlKey: true, altKey: true, getModifierState: altGraph(true) },
        "win",
      ),
    ).toBeNull();
  });

  it("adds nothing for a key with no letter or digit position", () => {
    expect(chordFromPosition({ key: "?", code: "Slash", metaKey: true }, "mac")).toBeNull();
  });
});

describe("chordForRecording", () => {
  it("records the position for a non-Latin letter", () => {
    expect(chordForRecording({ key: "ى", code: "KeyN", ctrlKey: true }, "win")).toBe(
      "CmdOrCtrl+N",
    );
  });

  it("records the digit for a shifted digit key", () => {
    expect(
      chordForRecording({ key: ")", code: "Digit0", metaKey: true, shiftKey: true }, "mac"),
    ).toBe("CmdOrCtrl+Shift+0");
  });

  it("records the character for unshifted punctuation and Latin letters", () => {
    expect(chordForRecording({ key: "\\", code: "Backslash", metaKey: true }, "mac")).toBe(
      "CmdOrCtrl+\\",
    );
    expect(chordForRecording({ key: "a", code: "KeyQ", metaKey: true }, "mac")).toBe(
      "CmdOrCtrl+A",
    );
  });
});

describe("Option+Space on a Mac", () => {
  // Before, the no-break space Option+Space types recorded as a chord no part
  // of Writ could read, so a chord with Option and Space could not be recorded.
  it("records as Space", () => {
    expect(
      chordForRecording(
        { key: "\u00a0", code: "Space", metaKey: true, shiftKey: true, altKey: true },
        "mac",
      ),
    ).toBe("CmdOrCtrl+Shift+Alt+Space");
  });
});

describe("sameChord", () => {
  it("matches the same keys whatever order the modifiers are written in", () => {
    expect(sameChord("CmdOrCtrl+Shift+Alt+Space", "CmdOrCtrl+Alt+Shift+Space")).toBe(true);
    expect(sameChord("CmdOrCtrl+Shift+N", "CmdOrCtrl+Shift+n")).toBe(true);
  });

  it("tells different keys and modifiers apart, and matches nothing to empty", () => {
    expect(sameChord("CmdOrCtrl+Shift+Space", "CmdOrCtrl+Shift+Alt+Space")).toBe(false);
    expect(sameChord("CmdOrCtrl+Space", "Ctrl+Space")).toBe(false);
    expect(sameChord("", "")).toBe(false);
  });
});

describe("chordParts", () => {
  it("keeps the plus key at the end of a chord", () => {
    expect(chordParts("CmdOrCtrl+Shift++")).toEqual(["CmdOrCtrl", "Shift", "+"]);
    expect(chordParts("CmdOrCtrl+N")).toEqual(["CmdOrCtrl", "N"]);
  });
});

describe("canonicalChord", () => {
  it("reads a Control chord recorded on a Mac as CmdOrCtrl elsewhere", () => {
    expect(canonicalChord("Ctrl+Shift+K", "win")).toBe("CmdOrCtrl+Shift+K");
    expect(canonicalChord("CmdOrCtrl+Ctrl+K", "linux")).toBe("CmdOrCtrl+K");
  });

  it("keeps Control apart from Command on macOS", () => {
    expect(canonicalChord("Ctrl+K", "mac")).toBe("Ctrl+K");
  });
});

describe("worksFromAnyApp", () => {
  it("refuses a key other apps type, Shift or not", () => {
    for (const binding of ["A", "Space", "F2", "Shift+A", "Shift+Space", "Shift+Shift"]) {
      expect(worksFromAnyApp(binding), binding).toBe(false);
    }
  });

  it("takes a chord with Cmd, Ctrl or Alt", () => {
    for (const binding of ["CmdOrCtrl+Shift+Space", "Ctrl+Space", "Cmd+K", "Alt+Space", "Shift+Alt+W"]) {
      expect(worksFromAnyApp(binding), binding).toBe(true);
    }
  });

  it("takes F13 to F24 on their own", () => {
    for (const binding of ["F13", "F20", "F24", "Shift+F13"]) {
      expect(worksFromAnyApp(binding), binding).toBe(true);
    }
    expect(worksFromAnyApp("F12")).toBe(false);
  });
});
