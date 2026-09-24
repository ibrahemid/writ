import { describe, expect, it } from "vitest";
import type { IpcBridge } from "../ipc";
import { createBackend, DemoCommandError } from "../backend/backend";
import { NOTES_ROOT } from "../backend/vfs";

function backend() {
  const emitted: { event: string; payload: unknown }[] = [];
  const sent: unknown[] = [];
  const bridge: IpcBridge = {
    emit: (event, payload) => emitted.push({ event, payload }),
    send: (_channel, _index, message) => sent.push(message),
    end: () => {},
  };
  return { handle: createBackend(bridge), emitted, sent };
}

describe("the demo backend", () => {
  it("opens with the hero capture's note as the only tab", async () => {
    const { handle } = backend();
    const tabs = (await handle("list_active_buffers", {})) as { source_path: string }[];
    expect(tabs.map((t) => t.source_path)).toEqual([`${NOTES_ROOT}/Garden committee 10 Sep.md`]);
  });

  it("lists folders before files, as the app's tree does", async () => {
    const { handle } = backend();
    const entries = (await handle("list_workspace_dir", { dirPath: NOTES_ROOT })) as { name: string; is_dir: boolean }[];
    const firstFile = entries.findIndex((e) => !e.is_dir);
    expect(entries.slice(firstFile).every((e) => !e.is_dir)).toBe(true);
    expect(entries.some((e) => e.name === "To do.txt")).toBe(true);
  });

  it("reports the digest the app computes for the same text", async () => {
    const { handle } = backend();
    const [doc] = (await handle("list_active_buffers", {})) as { id: string }[];
    const saved = await handle("save_buffer_content", { id: doc.id, content: "writ\r\n" });
    const { hashDocument } = await import("../../src/lib/doc-hash");
    expect(saved).toBe(await hashDocument("writ\n"));
  });

  it("writes a new file on its first save and renames it from its first line", async () => {
    const { handle, emitted } = backend();
    const doc = (await handle("create_buffer", { title: null })) as { id: string; filename: string };
    expect(doc.filename).toMatch(/^writ-\d{6}-\d{4}\.txt$/);
    await handle("save_buffer_content", { id: doc.id, content: "Packing for Lisbon\nsocks" });
    const outcome = (await handle("auto_retitle_note", { id: doc.id })) as { kind: string; note?: { source_path: string } };
    expect(outcome.kind).toBe("renamed");
    expect(outcome.note?.source_path).toBe(`${NOTES_ROOT}/Packing for Lisbon.txt`);
    expect(emitted.some((e) => e.event === "writ://notes-changed")).toBe(true);
  });

  it("finds content across .txt and .md files and streams it on the channel", async () => {
    const { handle, sent } = backend();
    await handle("search_workspace_content", { query: "compost", onBatch: { id: 1 } });
    const batch = sent[0] as { hits: { path: string }[]; outcome: { cancelled: boolean } };
    expect(batch.hits.some((h) => h.path.endsWith(".txt"))).toBe(true);
    expect(batch.hits.some((h) => h.path.endsWith(".md"))).toBe(true);
    expect(batch.outcome.cancelled).toBe(false);
  });

  it("resolves a wikilink by file name", async () => {
    const { handle } = backend();
    const answer = (await handle("resolve_note_link", { fromPath: "", target: "Seed order" })) as { status: string; path: string };
    expect(answer.status).toBe("resolved");
    expect(answer.path).toBe(`${NOTES_ROOT}/Garden/Seed order.md`);
  });

  it("refuses a command it has no answer for by name", () => {
    const { handle } = backend();
    expect(() => handle("no_such_command", {})).toThrow(DemoCommandError);
  });
});
