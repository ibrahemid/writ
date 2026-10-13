import { describe, expect, it } from "vitest";
import type { BufferDocument, FileOpenResult } from "../../src/types/buffer";
import type { IpcBridge } from "../ipc";
import { classifySize, ERR_OPEN_TOO_LARGE, formatHexDump } from "../backend/classify";
import { createBufferCommands } from "../backend/commands/buffers";
import { DemoState } from "../backend/state";
import { NOTES_ROOT } from "../backend/vfs";

const MiB = 1024 * 1024;

function demo() {
  const bridge: IpcBridge = { emit: () => {}, send: () => {}, end: () => {} };
  const state = new DemoState(bridge);
  const commands = createBufferCommands(state);
  const open = async (command: "open_file" | "open_file_confirmed", path: string) =>
    (await commands[command]({ path })) as FileOpenResult;
  return { state, commands, open };
}

describe("the demo opens a file the way open_file does", () => {
  it("titles the tab with the file name, extension included", async () => {
    const { state, open } = demo();
    const path = `${NOTES_ROOT}/notes-4.9MiB.md`;
    state.folder.write(path, "# Heading\n");
    const opened = await open("open_file", path);
    expect(opened.doc?.title).toBe("notes-4.9MiB.md");
    expect(opened.mode).toEqual({ kind: "Normal" });
  });

  it("opens a file over 5 MiB as a large file", async () => {
    const { state, open } = demo();
    const path = `${NOTES_ROOT}/big.log`;
    state.folder.write(path, "x".repeat(5 * MiB + 1));
    const opened = await open("open_file", path);
    expect(opened.mode).toEqual({ kind: "LargeFile" });
    expect(opened.size_bytes).toBe(5 * MiB + 1);
    expect(opened.doc?.size_bytes).toBe(5 * MiB + 1);
  });

  it("asks before a file over 50 MiB, and opens it once the person says Open", async () => {
    const { state, open } = demo();
    const path = `${NOTES_ROOT}/huge.log`;
    state.folder.write(path, "y".repeat(50 * MiB + 1));
    const asked = await open("open_file", path);
    expect(asked).toEqual({ doc: null, mode: { kind: "LargeFileConfirm" }, size_bytes: 50 * MiB + 1 });
    const confirmed = await open("open_file_confirmed", path);
    expect(confirmed.mode).toEqual({ kind: "LargeFile" });
    expect(confirmed.doc?.source_path).toBe(path);
    const again = await open("open_file", path);
    expect(again.mode).toEqual({ kind: "LargeFile" });
    expect(again.doc?.id).toBe(confirmed.doc?.id);
  });

  it("opens a file with a NUL in its first 8 KiB read-only, as the hex dump Rust shows", async () => {
    const { state, commands, open } = demo();
    const path = `${NOTES_ROOT}/blob.bin`;
    state.folder.write(path, "AB\u0000C");
    const opened = await open("open_file", path);
    expect(opened.mode).toEqual({ kind: "Binary" });
    const doc = opened.doc as BufferDocument;
    expect(doc.read_only).toBe(true);
    const bytes = (await commands.read_buffer_content({ id: doc.id })) as ArrayBuffer;
    expect(new TextDecoder().decode(bytes)).toBe(
      "00000000  41 42 00 43                                       |AB.C|\n",
    );
  });

  it("reads a NUL past the first 8 KiB as text", async () => {
    const { state, open } = demo();
    const path = `${NOTES_ROOT}/late-nul.txt`;
    state.folder.write(path, `${"z".repeat(8192)}\u0000`);
    expect((await open("open_file", path)).mode).toEqual({ kind: "Normal" });
  });
});

describe("the demo's copy of file_ops", () => {
  it("refuses a file past the size Writ opens, naming the code first", () => {
    const mode = classifySize(500 * MiB + 1, false);
    expect(mode).toEqual({ kind: "Refused", reason: `${ERR_OPEN_TOO_LARGE}: 500.0 MiB is over the 500.0 MiB limit` });
    expect(classifySize(500 * MiB, false)).toEqual({ kind: "LargeFileConfirm" });
    expect(classifySize(10, true)).toEqual({ kind: "Binary" });
  });

  it("lays out a hex dump the way generate_hex_dump does", () => {
    const bytes = (values: number[]) => new Uint8Array(values);
    expect(formatHexDump(bytes([]), 0)).toBe("");
    const letters = formatHexDump(bytes(Array.from({ length: 16 }, (_, i) => 0x41 + i)), 16);
    expect(letters).toBe("00000000  41 42 43 44 45 46 47 48  49 4a 4b 4c 4d 4e 4f 50  |ABCDEFGHIJKLMNOP|\n");
    const rows = formatHexDump(bytes(Array.from({ length: 17 }, (_, i) => i)), 17).split("\n");
    expect(rows[1].startsWith("00000010")).toBe(true);
    expect(formatHexDump(bytes([0x00, 0x01, 0x41, 0x7f]), 4)).toContain("|..A.|");
    const truncated = formatHexDump(bytes([0x41]), 10 * MiB + 1);
    expect(truncated.endsWith("\n[ truncated — showing first 10.0 MiB of 10.0 MiB ]\n")).toBe(true);
  });

  it("brings a closed tab back in the tier its file is in now", async () => {
    const { state, commands, open } = demo();
    const path = `${NOTES_ROOT}/grows.log`;
    state.folder.write(path, "small");
    const opened = await open("open_file", path);
    const id = (opened.doc as BufferDocument).id;
    await commands.close_buffer({ id });
    state.folder.write(path, "g".repeat(50 * MiB + 1));
    const asked = (await commands.restore_buffer({ id })) as FileOpenResult;
    expect(asked).toEqual({ doc: null, mode: { kind: "LargeFileConfirm" }, size_bytes: 50 * MiB + 1 });
    const restored = (await commands.restore_buffer_confirmed({ id })) as FileOpenResult;
    expect(restored.mode).toEqual({ kind: "LargeFile" });
    expect(restored.doc).toMatchObject({ id, status: "active", size_bytes: 50 * MiB + 1 });
  });
});
