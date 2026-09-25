import { describe, expect, it } from "vitest";
import type { IpcBridge } from "../ipc";
import { formatRenameError } from "../../src/lib/save-error";
import { createBackend, DemoCommandError } from "../backend/backend";
import { SEED_FILES, SEED_VERSIONS } from "../backend/seed";
import { VersionsNotSeededError } from "../backend/state";
import { NOTES_ROOT } from "../backend/vfs";

function backend() {
  const emitted: { event: string; payload: unknown }[] = [];
  const sent: unknown[] = [];
  const bridge: IpcBridge = {
    emit: (event, payload) => emitted.push({ event, payload }),
    send: (_channel, _index, message) => sent.push(message),
    end: () => {},
  };
  const { handle, controls } = createBackend(bridge);
  return { handle, controls, emitted, sent };
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

  it("names content hits relative to the workspace root, as workspace_grep does", async () => {
    const { handle, sent } = backend();
    await handle("search_workspace_content", { query: "compost", onBatch: { id: 1 } });
    const batch = sent[0] as { hits: { path: string }[] };
    expect(batch.hits.length).toBeGreaterThan(0);
    expect(batch.hits.every((h) => !h.path.startsWith("/"))).toBe(true);
  });

  it("refuses a content search with no workspace folder open", async () => {
    const { handle } = backend();
    await handle("clear_workspace_root", {});
    await expect(handle("search_workspace_content", { query: "compost", onBatch: { id: 1 } })).rejects.toBe(
      "no workspace folder is open",
    );
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

  it("rejects an id no tab holds in the words the store uses, sync or async", async () => {
    const { handle } = backend();
    await expect(handle("get_buffer", { id: "demo-999" })).rejects.toBe("consistency error: buffer not found: demo-999");
    await expect(handle("note_disk_state", { id: "demo-999" })).rejects.toBe(
      "consistency error: buffer not found: demo-999",
    );
  });

  it("finds files by fuzzy name, best match first, and only in the notes folder", async () => {
    const { handle } = backend();
    const hits = (await handle("search_workspace_files", { query: "seed" })) as { path: string; name: string; score: number }[];
    expect(hits.length).toBeGreaterThan(0);
    expect(hits[0].name).toBe("Seed order.md");
    expect(hits.every((hit) => hit.path.startsWith(`${NOTES_ROOT}/`))).toBe(true);
    expect(hits.map((hit) => hit.score)).toEqual([...hits.map((hit) => hit.score)].sort((a, b) => b - a));
    expect(await handle("search_workspace_files", { query: "qqqq" })).toEqual([]);
  });

  it("answers the notes index from the folder: facts, backlinks, graph and tags", async () => {
    const { handle } = backend();
    const seed = `${NOTES_ROOT}/Garden/Seed order.md`;
    const facts = (await handle("note_facts", { path: seed })) as { properties: { key: string }[]; tags: { tag: string }[] };
    expect(facts.properties.map((p) => p.key)).toContain("supplier");
    expect(facts.tags.map((t) => t.tag)).toContain("project/garden");
    const backlinks = (await handle("note_backlinks", { path: seed })) as { from_name: string }[];
    expect(backlinks.map((b) => b.from_name)).toContain("Garden committee 10 Sep");
    const graph = (await handle("note_graph", {})) as { nodes: { path: string }[]; edges: unknown[] };
    expect(graph.nodes.every((n) => n.path.endsWith(".md"))).toBe(true);
    expect(graph.edges.length).toBeGreaterThan(0);
    const tags = (await handle("note_all_tags", {})) as { tag: string; count: number }[];
    expect(tags[0].count).toBeGreaterThanOrEqual(tags[tags.length - 1].count);
  });

  it("renames a note and rewrites the links that reached it", async () => {
    const { handle } = backend();
    const plan = `${NOTES_ROOT}/Garden/Garden plan.md`;
    expect(await handle("count_links_to", { path: plan })).toBeGreaterThan(0);
    const outcome = (await handle("rename_note_with_links", { path: plan, newName: "Allotment", updateLinks: true })) as {
      renamed_path: string;
      updated_paths: string[];
    };
    expect(outcome.renamed_path).toBe(`${NOTES_ROOT}/Garden/Allotment.md`);
    const [first] = outcome.updated_paths;
    const doc = (await handle("open_file", { path: first })) as { doc: { id: string } };
    const bytes = (await handle("read_buffer_content", { id: doc.doc.id })) as ArrayBuffer;
    expect(new TextDecoder().decode(bytes)).toContain("Allotment");
    expect(await handle("count_links_to", { path: `${NOTES_ROOT}/Garden/Allotment.md` })).toBe(outcome.updated_paths.length);
  });

  it("rejects a taken name with the app's own sentence", async () => {
    const { handle } = backend();
    const [doc] = (await handle("list_active_buffers", {})) as { id: string }[];
    await expect(handle("rename_note", { id: doc.id, title: "Birthday ideas" })).rejects.toBe('A file named "Birthday ideas.md" is already there.');
  });

  it("refuses a rename of a file that is gone with the code the app words itself", async () => {
    const { handle } = backend();
    const refusal = await (handle("rename_note_with_links", {
      path: `${NOTES_ROOT}/Gone.md`,
      newName: "Back",
      updateLinks: false,
    }) as Promise<unknown>).catch((reason: unknown) => reason);
    expect(refusal).toMatch(/^ERR_FILE_MISSING: /);
    expect(formatRenameError(refusal)).toBe("The file could not be renamed: the folder this file was in is no longer there.");
  });

  it("keeps versions of a saved note and restores one", async () => {
    const { handle, emitted } = backend();
    const [doc] = (await handle("list_active_buffers", {})) as { id: string; source_path: string }[];
    await handle("save_buffer_content", { id: doc.id, content: "changed" });
    const versions = (await handle("note_versions", { path: doc.source_path })) as { id: number }[];
    expect(versions).toHaveLength(2);
    const original = versions[1];
    expect(await handle("note_version_content", { versionId: original.id })).toContain("Garden committee");
    await handle("restore_note_version", { versionId: original.id });
    expect(emitted.some((e) => e.event === "writ://buffer-external")).toBe(true);
    await expect(handle("note_version_content", { versionId: 999 })).rejects.toBe("That version is not here any more.");
  });

  it("lists three earlier versions of the newsletter and restores the second one's text", async () => {
    const { handle } = backend();
    const path = `${NOTES_ROOT}/Newsletter draft.md`;
    const versions = (await handle("note_versions", { path })) as { id: number; at_ms: number }[];
    expect(versions).toHaveLength(3);
    expect(versions.map((v) => v.at_ms)).toEqual([...versions.map((v) => v.at_ms)].sort((a, b) => b - a));
    const second = (await handle("note_version_content", { versionId: versions[1].id })) as string;
    expect(second).toContain("Shakshuka, probably.");
    await handle("restore_note_version", { versionId: versions[1].id });
    const doc = (await handle("open_file", { path })) as { doc: { id: string } };
    const bytes = (await handle("read_buffer_content", { id: doc.doc.id })) as Uint8Array;
    expect(new TextDecoder().decode(bytes)).toBe(second);
  });

  it("resets the newsletter to its three seeded versions and seed text after a restore", async () => {
    const { handle, controls, emitted } = backend();
    const path = `${NOTES_ROOT}/Newsletter draft.md`;
    const opened = (await handle("open_file", { path })) as { doc: { id: string } };
    const before = (await handle("note_versions", { path })) as { id: number }[];
    await handle("restore_note_version", { versionId: before[1].id });
    expect(await handle("note_versions", { path })).toHaveLength(5);
    emitted.length = 0;

    await controls.resetVersions(path);

    const after = (await handle("note_versions", { path })) as { id: number }[];
    expect(after).toHaveLength(3);
    const texts = await Promise.all(after.map((v) => handle("note_version_content", { versionId: v.id })));
    expect(texts).toEqual(SEED_VERSIONS.map((version) => version.text).reverse());
    const bytes = (await handle("read_buffer_content", { id: opened.doc.id })) as ArrayBuffer;
    expect(new TextDecoder().decode(bytes)).toBe(SEED_FILES["Newsletter draft.md"]);
    expect(emitted.filter((e) => e.event === "writ://buffer-external")).toHaveLength(1);

    await controls.resetVersions(path);
    expect(await handle("note_versions", { path })).toHaveLength(3);
    expect(emitted.filter((e) => e.event === "writ://buffer-external")).toHaveLength(1);
    await expect(controls.resetVersions(`${NOTES_ROOT}/To do.txt`)).rejects.toBeInstanceOf(VersionsNotSeededError);
  });

  it("reports the connection honestly: nothing local answers, no key is held", async () => {
    const { handle } = backend();
    expect(await handle("ai_probe_local", {})).toEqual({ ollama: false, lmstudio: false });
    expect(await handle("ai_has_api_key", { provider: "anthropic" })).toEqual({ is_set: false, memory_only: false });
    expect(((await handle("ai_check_connection", {})) as { kind: string }).kind).toBe("refused");
    await expect(handle("ai_rewrite", { requestId: "r1", action: "polish", text: "x" })).rejects.toBe("Rewriting is turned off.");
    await expect(handle("ai_consent_host", {})).rejects.toBe("This endpoint is on your machine; nothing is sent.");
  });

  it("holds chats for the page and fails a send the way an offline server does", async () => {
    const { handle, emitted } = backend();
    const config = (await handle("get_config", {})) as { ai: { chat: { enabled: boolean }; model: string } };
    config.ai.chat.enabled = true;
    config.ai.model = "qwen3:4b";
    await handle("update_config", { config });
    const chat = (await handle("chat_new", {})) as { id: string };
    await handle("chat_send", { conversationId: chat.id, text: "What is due?", contextPaths: [], requestId: "q1" });
    await new Promise((resolve) => setTimeout(resolve, 0));
    const frame = emitted.find((e) => e.event === "writ://ai-chat")?.payload as { payload: { error: { kind: string } } };
    expect(frame.payload.error.kind).toBe("local_server_offline");
    const list = (await handle("chat_list", {})) as { title: string; turns: number }[];
    expect(list).toEqual([expect.objectContaining({ title: "What is due?", turns: 1 })]);
    await handle("chat_delete", { id: chat.id });
    await expect(handle("chat_open", { id: chat.id })).rejects.toBe("This chat no longer exists.");
  });

  it("creates the note a missing link names, and today's note once", async () => {
    const { handle } = backend();
    const made = (await handle("new_note_from_link", { target: "Ideas/Seed swap.md" })) as { source_path: string };
    expect(made.source_path).toBe(`${NOTES_ROOT}/Ideas/Seed swap.md`);
    const today = (await handle("todays_note", {})) as { id: string; source_path: string };
    const again = (await handle("todays_note", {})) as { id: string };
    expect(again.id).toBe(today.id);
    expect(today.source_path).toMatch(/\/\d{4}-\d{2}-\d{2}\.txt$/);
  });
});

