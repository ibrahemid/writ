import { createSignal } from "solid-js";
import type { BufferDocument } from "../../types/buffer";
import type { BufferRegistry } from "../global/buffer-registry";

export type TabStore = ReturnType<typeof createTabStore>;

// Per-window selected-tab state. The set of buffers is app-global
// (bufferRegistry); which one is focused is per-window. Tab-management
// operations are surfaced here so the per-window activeTabId tracks
// registry mutations atomically.

export function createTabStore(deps: { registry: BufferRegistry }) {
  const { registry } = deps;
  const [activeTabId, setActiveTabId] = createSignal<string | null>(null);

  async function loadAndActivate() {
    await registry.load();
    const active = registry.activeTabs();
    const currentId = activeTabId();
    if (currentId && !active.find((b) => b.id === currentId)) {
      setActiveTabId(active.length > 0 ? active[active.length - 1].id : null);
    } else if (!currentId && active.length > 0) {
      setActiveTabId(active[active.length - 1].id);
    }
  }

  async function createTab(title?: string): Promise<BufferDocument> {
    const doc = await registry.createBuffer(title);
    setActiveTabId(doc.id);
    return doc;
  }

  async function closeTab(id: string): Promise<void> {
    await registry.closeBuffer(id);
    if (activeTabId() === id) {
      const remaining = registry.activeTabs();
      setActiveTabId(remaining.length > 0 ? remaining[remaining.length - 1].id : null);
    }
  }

  async function closeOtherTabs(keepId: string): Promise<void> {
    const toClose = registry.activeTabs().filter((b) => b.id !== keepId);
    if (toClose.length === 0) return;
    await registry.closeBuffers(toClose.map((b) => b.id));
    setActiveTabId(keepId);
  }

  async function closeAllTabs(): Promise<void> {
    const toClose = registry.activeTabs();
    if (toClose.length === 0) {
      setActiveTabId(null);
      return;
    }
    await registry.closeBuffers(toClose.map((b) => b.id));
    setActiveTabId(null);
  }

  async function restoreFromHistory(id: string): Promise<void> {
    await registry.restoreBuffer(id);
    setActiveTabId(id);
  }

  async function openFile(path: string): Promise<BufferDocument> {
    const { doc } = await registry.openFile(path);
    setActiveTabId(doc.id);
    return doc;
  }

  async function openFileDialog(): Promise<BufferDocument | null> {
    const path = await registry.showOpenFileDialog();
    if (!path) return null;
    return openFile(path);
  }

  return {
    activeTabId,
    setActiveTabId,
    loadAndActivate,
    createTab,
    closeTab,
    closeOtherTabs,
    closeAllTabs,
    restoreFromHistory,
    openFile,
    openFileDialog,
  };
}
