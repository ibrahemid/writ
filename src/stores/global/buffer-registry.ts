import { createSignal, createMemo, createRoot } from "solid-js";
import type { BufferDocument } from "../../types/buffer";
import * as api from "../../services/tauri";
import { flushAutosave } from "../../services/autosave";

// Singleton — app-global, not window-scoped (ADR-009 E3).
// The set of buffers is shared across every window; active-tab focus per
// window lives in stores/window/tab-store.ts.

function createBufferRegistry() {
  const [buffers, setBuffers] = createSignal<BufferDocument[]>([]);

  const activeTabs = createMemo(() =>
    buffers().filter((b) => b.status === "active"),
  );
  const historyList = createMemo(() =>
    buffers().filter((b) => b.status === "history"),
  );

  async function load() {
    const [active, history] = await Promise.all([
      api.listActiveBuffers(),
      api.listHistory(),
    ]);
    setBuffers([...active, ...history]);
  }

  async function createBuffer(title?: string): Promise<BufferDocument> {
    await flushAutosave();
    const doc = await api.createBuffer(title);
    setBuffers((prev) =>
      prev.find((b) => b.id === doc.id) ? prev : [...prev, doc],
    );
    return doc;
  }

  async function closeBuffer(id: string): Promise<void> {
    await flushAutosave(id);
    await api.closeBuffer(id);
    const closedAt = new Date().toISOString();
    setBuffers((prev) =>
      prev.map((b) =>
        b.id === id ? { ...b, status: "history" as const, closed_at: closedAt } : b,
      ),
    );
  }

  async function closeBuffers(ids: string[]): Promise<void> {
    if (ids.length === 0) return;
    await Promise.all(ids.map((id) => flushAutosave(id)));
    await api.closeBuffers(ids);
    const closedAt = new Date().toISOString();
    const closedIds = new Set(ids);
    setBuffers((prev) =>
      prev.map((b) =>
        closedIds.has(b.id)
          ? { ...b, status: "history" as const, closed_at: closedAt }
          : b,
      ),
    );
  }

  async function restoreBuffer(id: string): Promise<void> {
    await api.restoreBuffer(id);
    await load();
  }

  async function deleteFromHistory(id: string): Promise<void> {
    await api.deleteBuffer(id);
    setBuffers((prev) => prev.filter((b) => b.id !== id));
  }

  async function clearAllHistory(): Promise<void> {
    await api.clearHistory();
    setBuffers((prev) => prev.filter((b) => b.status !== "history"));
  }

  async function renameBuffer(id: string, title: string): Promise<void> {
    await api.renameBuffer(id, title);
    setBuffers((prev) => prev.map((b) => (b.id === id ? { ...b, title } : b)));
  }

  async function openFile(path: string): Promise<{ doc: BufferDocument; existed: boolean }> {
    const existing = activeTabs().find((b) => b.source_path === path);
    if (existing) return { doc: existing, existed: true };
    const doc = await api.openFile(path);
    setBuffers((prev) => {
      if (prev.find((b) => b.id === doc.id)) {
        return prev.map((b) => (b.id === doc.id ? doc : b));
      }
      return [...prev, doc];
    });
    return { doc, existed: false };
  }

  async function showOpenFileDialog(): Promise<string | null> {
    return api.showOpenFileDialog();
  }

  async function readContent(id: string): Promise<string> {
    return api.readBufferContent(id);
  }

  return {
    buffers,
    activeTabs,
    historyList,
    load,
    createBuffer,
    closeBuffer,
    closeBuffers,
    restoreBuffer,
    deleteFromHistory,
    clearAllHistory,
    renameBuffer,
    openFile,
    showOpenFileDialog,
    readContent,
  };
}

export const bufferRegistry = createRoot(createBufferRegistry);
export type BufferRegistry = ReturnType<typeof createBufferRegistry>;
