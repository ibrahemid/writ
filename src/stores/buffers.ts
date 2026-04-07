import { createSignal, createRoot } from "solid-js";
import type { BufferDocument } from "../types/buffer";
import * as api from "../services/tauri";

function createBufferStore() {
  const [activeTabs, setActiveTabs] = createSignal<BufferDocument[]>([]);
  const [historyList, setHistoryList] = createSignal<BufferDocument[]>([]);
  const [activeTabId, setActiveTabId] = createSignal<string | null>(null);

  async function load() {
    const [active, history] = await Promise.all([
      api.listActiveBuffers(),
      api.listHistory(),
    ]);
    setActiveTabs(active);
    setHistoryList(history);
  }

  async function loadAndActivate() {
    await load();
    const active = activeTabs();
    const currentId = activeTabId();
    if (currentId && !active.find(b => b.id === currentId)) {
      setActiveTabId(active.length > 0 ? active[active.length - 1].id : null);
    } else if (!currentId && active.length > 0) {
      setActiveTabId(active[active.length - 1].id);
    }
  }

  async function createTab(title?: string): Promise<BufferDocument> {
    const doc = await api.createBuffer(title);
    setActiveTabs(prev => [...prev, doc]);
    setActiveTabId(doc.id);
    return doc;
  }

  async function closeTab(id: string) {
    const tab = activeTabs().find(b => b.id === id);
    await api.closeBuffer(id);
    setActiveTabs(prev => prev.filter(b => b.id !== id));
    if (tab) {
      setHistoryList(prev => [{ ...tab, status: "history" as const, closed_at: new Date().toISOString() }, ...prev]);
    }
    if (activeTabId() === id) {
      const remaining = activeTabs().filter(b => b.id !== id);
      setActiveTabId(remaining.length > 0 ? remaining[remaining.length - 1].id : null);
    }
  }

  async function closeOtherTabs(keepId: string) {
    const toClose = activeTabs().filter(b => b.id !== keepId);
    for (const tab of toClose) {
      await api.closeBuffer(tab.id);
    }
    setActiveTabs(prev => prev.filter(b => b.id === keepId));
    setActiveTabId(keepId);
    await load();
  }

  async function closeAllTabs() {
    for (const tab of activeTabs()) {
      await api.closeBuffer(tab.id);
    }
    setActiveTabs([]);
    setActiveTabId(null);
    await load();
  }

  async function restoreFromHistory(id: string) {
    await api.restoreBuffer(id);
    await load();
    setActiveTabId(id);
  }

  async function deleteFromHistory(id: string) {
    await api.deleteBuffer(id);
    setHistoryList(prev => prev.filter(b => b.id !== id));
  }

  async function clearAllHistory() {
    await api.clearHistory();
    setHistoryList([]);
  }

  async function renameTab(id: string, title: string) {
    await api.renameBuffer(id, title);
    setActiveTabs(prev => prev.map(b => b.id === id ? { ...b, title } : b));
    setHistoryList(prev => prev.map(b => b.id === id ? { ...b, title } : b));
  }

  async function openFile(path: string): Promise<BufferDocument> {
    const existing = activeTabs().find(b => b.source_path === path);
    if (existing) {
      setActiveTabId(existing.id);
      return existing;
    }
    const doc = await api.openFile(path);
    const alreadyInTabs = activeTabs().find(b => b.id === doc.id);
    if (alreadyInTabs) {
      setActiveTabId(doc.id);
      return doc;
    }
    setActiveTabs(prev => [...prev, doc]);
    setActiveTabId(doc.id);
    return doc;
  }

  async function openFileDialog(): Promise<BufferDocument | null> {
    const path = await api.showOpenFileDialog();
    if (!path) return null;
    return openFile(path);
  }

  return {
    activeTabs, historyList, activeTabId, setActiveTabId,
    load, loadAndActivate, createTab, closeTab, closeOtherTabs, closeAllTabs,
    restoreFromHistory, deleteFromHistory, clearAllHistory, renameTab,
    openFile, openFileDialog,
  };
}

export const bufferStore = createRoot(createBufferStore);
