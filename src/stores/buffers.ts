import { createSignal, createMemo, createRoot } from "solid-js";
import type { BufferDocument } from "../types/buffer";
import * as api from "../services/tauri";
import { flushAutosave } from "../services/autosave";

function createBufferStore() {
  const [buffers, setBuffers] = createSignal<BufferDocument[]>([]);
  const [activeTabId, setActiveTabId] = createSignal<string | null>(null);

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

  async function loadAndActivate() {
    await load();
    const active = activeTabs();
    const currentId = activeTabId();
    if (currentId && !active.find((b) => b.id === currentId)) {
      setActiveTabId(active.length > 0 ? active[active.length - 1].id : null);
    } else if (!currentId && active.length > 0) {
      setActiveTabId(active[active.length - 1].id);
    }
  }

  async function createTab(title?: string): Promise<BufferDocument> {
    await flushAutosave();
    const doc = await api.createBuffer(title);
    setBuffers((prev) =>
      prev.find((b) => b.id === doc.id) ? prev : [...prev, doc],
    );
    setActiveTabId(doc.id);
    return doc;
  }

  async function closeTab(id: string) {
    await flushAutosave(id);
    await api.closeBuffer(id);
    const closedAt = new Date().toISOString();
    setBuffers((prev) =>
      prev.map((b) =>
        b.id === id ? { ...b, status: "history" as const, closed_at: closedAt } : b,
      ),
    );
    if (activeTabId() === id) {
      const remaining = activeTabs();
      setActiveTabId(remaining.length > 0 ? remaining[remaining.length - 1].id : null);
    }
  }

  async function closeOtherTabs(keepId: string) {
    const toClose = activeTabs().filter((b) => b.id !== keepId);
    if (toClose.length === 0) return;
    const ids = toClose.map((b) => b.id);
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
    setActiveTabId(keepId);
  }

  async function closeAllTabs() {
    const toClose = activeTabs();
    if (toClose.length === 0) {
      setActiveTabId(null);
      return;
    }
    const ids = toClose.map((b) => b.id);
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
    setActiveTabId(null);
  }

  async function restoreFromHistory(id: string) {
    await api.restoreBuffer(id);
    await load();
    setActiveTabId(id);
  }

  async function deleteFromHistory(id: string) {
    await api.deleteBuffer(id);
    setBuffers((prev) => prev.filter((b) => b.id !== id));
  }

  async function clearAllHistory() {
    await api.clearHistory();
    setBuffers((prev) => prev.filter((b) => b.status !== "history"));
  }

  async function renameTab(id: string, title: string) {
    await api.renameBuffer(id, title);
    setBuffers((prev) => prev.map((b) => (b.id === id ? { ...b, title } : b)));
  }

  async function openFile(path: string): Promise<BufferDocument> {
    const existing = activeTabs().find((b) => b.source_path === path);
    if (existing) {
      setActiveTabId(existing.id);
      return existing;
    }
    const doc = await api.openFile(path);
    setBuffers((prev) => {
      if (prev.find((b) => b.id === doc.id)) {
        return prev.map((b) => (b.id === doc.id ? doc : b));
      }
      return [...prev, doc];
    });
    setActiveTabId(doc.id);
    return doc;
  }

  async function openFileDialog(): Promise<BufferDocument | null> {
    const path = await api.showOpenFileDialog();
    if (!path) return null;
    return openFile(path);
  }

  async function readContent(id: string): Promise<string> {
    return api.readBufferContent(id);
  }

  return {
    activeTabs,
    historyList,
    activeTabId,
    setActiveTabId,
    load,
    loadAndActivate,
    createTab,
    closeTab,
    closeOtherTabs,
    closeAllTabs,
    restoreFromHistory,
    deleteFromHistory,
    clearAllHistory,
    renameTab,
    openFile,
    openFileDialog,
    readContent,
  };
}

export const bufferStore = createRoot(createBufferStore);
