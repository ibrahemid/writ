import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => "digest"),
}));

import { invoke } from "@tauri-apps/api/core";
import { saveBufferChanges, saveBufferContent, setBufferViewPosition } from "../tauri";

const mockedInvoke = vi.mocked(invoke);

describe("saveBufferContent", () => {
  beforeEach(() => {
    mockedInvoke.mockClear();
  });

  it("sends the text as UTF-8 bytes with the note in a header, not as JSON", async () => {
    const text = "café · 中文 · مرحبا · 😀👍🏽\nline two\n";

    expect(await saveBufferContent("note-1", text)).toBe("digest");

    const [command, body, options] = mockedInvoke.mock.calls[0];
    expect(command).toBe("save_buffer_content");
    expect(ArrayBuffer.isView(body)).toBe(true);
    expect(new TextDecoder().decode(body as Uint8Array)).toBe(text);
    expect(options).toEqual({ headers: { "writ-note-id": "note-1" } });
  });
});

describe("saveBufferChanges", () => {
  it("names the base and the length of the document the changes produce", async () => {
    await saveBufferChanges("note-1", "abc", [3, [0, "x"]], 4);
    expect(mockedInvoke).toHaveBeenLastCalledWith("save_buffer_changes", {
      id: "note-1",
      base: "abc",
      changes: [3, [0, "x"]],
      length: 4,
    });
  });
});

describe("setBufferViewPosition", () => {
  it("sends the caret and the top of the view", async () => {
    await setBufferViewPosition("note-1", 12, 3);
    expect(mockedInvoke).toHaveBeenLastCalledWith("set_buffer_view_position", {
      id: "note-1",
      cursorPos: 12,
      scrollPos: 3,
    });
  });
});
