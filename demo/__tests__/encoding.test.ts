import { describe, expect, it } from "vitest";
import type { IpcBridge } from "../ipc";
import { createBackend } from "../backend/backend";
import {
  EncodingLossyError,
  EncodingUnwritableError,
  NotInEncodingError,
  decodeIn,
  encodeIn,
  resolveEncoding,
} from "../backend/encoding";
import { HERO_NOTE, SEED_FILES } from "../backend/seed";
import { NOTES_ROOT } from "../backend/vfs";
import { ERR_READ_FILE_MISSING, ERR_READ_NOT_UTF8, readFailureCodeOf } from "../../src/lib/read-failure";
import { lossyCharacters } from "../../src/lib/save-error";
import type { BufferDocument } from "../../src/types/buffer";

const LISBON = "Lisbon in October.md";

function backend() {
  const bridge: IpcBridge = { emit: () => {}, send: () => {}, end: () => {} };
  return createBackend(bridge).handle;
}

type Handle = ReturnType<typeof backend>;

async function open(handle: Handle, relative: string): Promise<BufferDocument> {
  const opened = (await handle("open_file", { path: `${NOTES_ROOT}/${relative}` })) as { doc: BufferDocument };
  return opened.doc;
}

/** What a command rejected with, or null when it resolved. */
function refusalOf(answer: unknown): Promise<unknown> {
  return Promise.resolve(answer).then(
    () => null,
    (error: unknown) => error,
  );
}

async function readText(handle: Handle, id: string): Promise<string> {
  return new TextDecoder().decode((await handle("read_buffer_content", { id })) as ArrayBuffer);
}

describe("the demo's text encodings", () => {
  it("resolves the names Reopen with Encoding sends as Rust does", () => {
    expect(resolveEncoding("windows-1252")).toBe("windows-1252");
    expect(resolveEncoding("latin1")).toBe("windows-1252");
    expect(resolveEncoding("shift_jis")).toBe("Shift_JIS");
    expect(resolveEncoding("macintosh")).toBe("macintosh");
    expect(resolveEncoding("UTF-8 BOM")).toBe("UTF-8 BOM");
    expect(resolveEncoding("windows-1252 BOM")).toBeNull();
    expect(resolveEncoding("replacement")).toBeNull();
    expect(resolveEncoding("klingon")).toBeNull();
  });

  it("decodes strictly and encodes back to the same bytes", () => {
    const utf8 = new TextEncoder().encode("café");
    expect(decodeIn(utf8, "windows-1252")).toBe("cafÃ©");
    expect([...encodeIn("cafÃ©", "windows-1252")]).toEqual([...utf8]);
    expect(decodeIn(utf8, "Shift_JIS")).toBe("cafﾃｩ");
    expect(() => decodeIn(Uint8Array.of(0x63, 0xe9), "UTF-8")).toThrow(NotInEncodingError);
    expect([...encodeIn("hi", "UTF-16LE BOM")]).toEqual([0xff, 0xfe, 0x68, 0x00, 0x69, 0x00]);
    expect([...encodeIn("plain", "Shift_JIS")]).toEqual([...new TextEncoder().encode("plain")]);
    expect(() => encodeIn("日本", "Shift_JIS")).toThrow(EncodingUnwritableError);
  });

  it("names the characters an encoding has no bytes for, in the shape the editor reads", () => {
    let thrown: unknown;
    try {
      encodeIn("a😀b中😀", "windows-1252");
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toBeInstanceOf(EncodingLossyError);
    expect(lossyCharacters(`ERR_ENCODING_LOSSY: ${(thrown as Error).message}`)).toEqual({
      encoding: "windows-1252",
      characters: ["😀", "中"],
    });
  });
});

describe("Reopen with Encoding on the demo", () => {
  // Before the demo answered it, this command threw DemoCommandError and the
  // page toasted that the file could not be reopened.
  it("reopens the hero note in another encoding and back", async () => {
    const handle = backend();
    const [hero] = (await handle("list_active_buffers", {})) as BufferDocument[];
    const reopened = (await handle("reopen_with_encoding", { bufferId: hero.id, encoding: "windows-1252" })) as BufferDocument;
    expect(reopened).toMatchObject({ id: hero.id, encoding: "windows-1252", line_ending: "lf" });
    expect(await readText(handle, hero.id)).toBe(SEED_FILES[HERO_NOTE]);
    const back = (await handle("reopen_with_encoding", { bufferId: hero.id, encoding: "UTF-8" })) as BufferDocument;
    expect(back.encoding).toBe("UTF-8");
  });

  it("shows the file's bytes read in the chosen encoding, and its own text again in UTF-8", async () => {
    const handle = backend();
    const doc = await open(handle, LISBON);
    await handle("reopen_with_encoding", { bufferId: doc.id, encoding: "windows-1252" });
    const misread = await readText(handle, doc.id);
    expect(misread).not.toBe(SEED_FILES[LISBON]);
    expect(misread).toContain("Ã");

    await handle("save_buffer_content", { id: doc.id, content: `${misread}more\n` });
    await handle("reopen_with_encoding", { bufferId: doc.id, encoding: "UTF-8" });
    expect(await readText(handle, doc.id)).toBe(`${SEED_FILES[LISBON]}more\n`);
  });

  it("refuses a save the encoding cannot hold, naming the character", async () => {
    const handle = backend();
    const doc = await open(handle, LISBON);
    await handle("reopen_with_encoding", { bufferId: doc.id, encoding: "windows-1252" });
    const refusal = await refusalOf(handle("save_buffer_content", { id: doc.id, content: "smile 😀" }));
    expect(lossyCharacters(refusal)).toEqual({ encoding: "windows-1252", characters: ["😀"] });
  });

  it("refuses an encoding the file is not text in, and the note keeps the one it had", async () => {
    const handle = backend();
    const doc = await open(handle, LISBON);
    await handle("reopen_with_encoding", { bufferId: doc.id, encoding: "windows-1252" });
    await handle("save_buffer_content", { id: doc.id, content: "café" });
    const refusal = await refusalOf(handle("reopen_with_encoding", { bufferId: doc.id, encoding: "UTF-8" }));
    expect(readFailureCodeOf(refusal)).toBe(ERR_READ_NOT_UTF8);
    expect(((await handle("get_buffer", { id: doc.id })) as BufferDocument).encoding).toBe("windows-1252");
    expect(await readText(handle, doc.id)).toBe("café");
  });

  it("refuses an unknown encoding and a note with no file", async () => {
    const handle = backend();
    const [hero] = (await handle("list_active_buffers", {})) as BufferDocument[];
    await expect(handle("reopen_with_encoding", { bufferId: hero.id, encoding: "klingon" })).rejects.toBe(
      'ERR_UNKNOWN_ENCODING: "klingon" is not an encoding',
    );
    const fresh = (await handle("create_buffer", {})) as BufferDocument;
    const refusal = await refusalOf(handle("reopen_with_encoding", { bufferId: fresh.id, encoding: "UTF-8" }));
    expect(readFailureCodeOf(refusal)).toBe(ERR_READ_FILE_MISSING);
  });
});
