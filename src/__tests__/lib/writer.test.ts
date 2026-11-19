import { describe, expect, it } from "vitest";

import { programName, readWriter, writerName } from "../../lib/writer";

describe("writer", () => {
  it("names each writer the way the version list shows it", () => {
    expect(writerName({ kind: "jotra" })).toBe("Jotra");
    expect(writerName({ kind: "chat" })).toBe("Chat");
    expect(writerName({ kind: "command" })).toBe("jotra command");
    expect(writerName({ kind: "program", name: "claude-code" })).toBe("Claude Code");
    expect(writerName({ kind: "program", name: "some-tool" })).toBe("some-tool");
  });

  it("names a program only for a program", () => {
    expect(programName({ kind: "program", name: "codex" })).toBe("Codex");
    expect(programName({ kind: "chat" })).toBeNull();
    expect(programName(null)).toBeNull();
    expect(programName(undefined)).toBeNull();
  });

  it("reads only the shapes Rust sends", () => {
    expect(readWriter({ kind: "jotra" })).toEqual({ kind: "jotra" });
    expect(readWriter({ kind: "program", name: "codex" })).toEqual({ kind: "program", name: "codex" });
    expect(readWriter({ kind: "program" })).toBeNull();
    expect(readWriter({ kind: "program", name: "  " })).toBeNull();
    expect(readWriter({ kind: "someone" })).toBeNull();
    expect(readWriter("jotra")).toBeNull();
    expect(readWriter(null)).toBeNull();
  });
});
