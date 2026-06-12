import { createSignal, createMemo, createRoot } from "solid-js";
import type { BufferDocument, FileOpenResult } from "../../types/buffer";
import * as api from "../../services/tauri";
import { flushAutosave } from "../../services/autosave";
import { requestConfirm } from "../../components/ConfirmDialog/ConfirmDialog";

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
    // The preview pane is a window-lifetime singleton (never unmounted, to
    // avoid the writ-preview:// iframe teardown freeze), so it no longer evicts
    // the host render cache per buffer. Evict here, at the close that ends the
    // buffer's session.
    void api.previewClose(id).catch(() => {});
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
    for (const id of ids) void api.previewClose(id).catch(() => {});
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

  function formatBytes(n: number): string {
    const GIB = 1024 * 1024 * 1024;
    const MIB = 1024 * 1024;
    const KIB = 1024;
    if (n >= GIB) return `${(n / GIB).toFixed(1)} GiB`;
    if (n >= MIB) return `${(n / MIB).toFixed(1)} MiB`;
    if (n >= KIB) return `${Math.round(n / KIB)} KiB`;
    return `${n} B`;
  }

  function registerOpenResult(result: FileOpenResult): { doc: BufferDocument; existed: boolean; mode: FileOpenResult["mode"] } {
    const doc = result.doc;
    setBuffers((prev) => {
      if (prev.find((b) => b.id === doc.id)) {
        return prev.map((b) => (b.id === doc.id ? doc : b));
      }
      return [...prev, doc];
    });
    return { doc, existed: false, mode: result.mode };
  }

  async function openFile(path: string): Promise<{ doc: BufferDocument; existed: boolean; mode: FileOpenResult["mode"] }> {
    const existing = activeTabs().find((b) => b.source_path === path);
    if (existing) {
      const mode = existing.read_only
        ? { kind: "Binary" as const }
        : existing.size_bytes > 50 * 1024 * 1024
        ? { kind: "LargeFile" as const }
        : existing.size_bytes > 5 * 1024 * 1024
        ? { kind: "LargeFile" as const }
        : { kind: "Normal" as const };
      return { doc: existing, existed: true, mode };
    }

    let result: FileOpenResult;
    try {
      result = await api.openFile(path);
    } catch (err) {
      const msg = String(err);
      if (msg.startsWith("__CONFIRM_REQUIRED__:")) {
        const parts = msg.split(":");
        const confirmedPath = parts[1];
        const sizeBytes = parseInt(parts[2], 10);
        const sizeStr = formatBytes(sizeBytes);
        const confirmed = await requestConfirm({
          title: "Open large file?",
          message: `This file is ${sizeStr}. Opening it will disable syntax highlighting, typography, and line wrapping. It will also be excluded from search and crash recovery.\n\nContinue?`,
          confirmLabel: "Open anyway",
          cancelLabel: "Cancel",
        });
        if (!confirmed) throw new Error("cancelled");
        result = await api.openFileConfirmed(confirmedPath);
      } else {
        throw err;
      }
    }

    return registerOpenResult(result);
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
