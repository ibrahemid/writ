import { createRoot, createSignal } from "solid-js";
import * as api from "../../services/tauri";
import type { ContextPack, PackSkipReason, PackSource } from "../../services/tauri";
import { collectUnsavedContent } from "../../services/autosave";
import { bufferRegistry } from "./buffer-registry";
import { notesStore } from "./notes";
import { clipboardStore } from "./clipboard";
import { showToast } from "./notifications";
import { windowRegistry } from "./window-registry";
import { dirname, isSameOrInside } from "../../lib/path";
import { logFailure } from "../../lib/log";

/** Which files a pack is made of. */
export type PackKind = "linked" | "folder" | "tag";

export type { ContextPack, PackSkipReason };

/**
 * Several files from the folder as one prompt, with a token count and a
 * preview: the file in front and the files it links to, the files of its
 * folder, or the files carrying a tag.
 *
 * Exported for tests; the app holds the one below.
 */
export function createContextPackStore() {
  const [isOpen, setOpen] = createSignal(false);
  const [kind, setKind] = createSignal<PackKind>("folder");
  // The file in front, when the folder holds it: the only file whose links
  // the index knows.
  const [file, setFile] = createSignal<string | null>(null);
  const [folder, setFolder] = createSignal<string | null>(null);
  const [tags, setTags] = createSignal<string[]>([]);
  const [tag, setTag] = createSignal<string | null>(null);
  const [pack, setPack] = createSignal<ContextPack | null>(null);
  const [busy, setBusy] = createSignal(false);
  const [failed, setFailed] = createSignal(false);
  let generation = 0;

  function sourceFor(chosen: PackKind): PackSource | null {
    switch (chosen) {
      case "linked": {
        const path = file();
        return path ? { kind: "linked", path } : null;
      }
      case "folder": {
        const path = folder();
        return path ? { kind: "folder", path } : null;
      }
      case "tag": {
        const name = tag();
        return name ? { kind: "tag", tag: name } : null;
      }
    }
  }

  /** What the open tabs hold that autosave has not written yet. */
  function liveTexts(): { path: string; text: string }[] {
    const pathOf = new Map(bufferRegistry.activeTabs().map((doc) => [doc.id, doc.source_path]));
    return collectUnsavedContent().flatMap(({ id, content }) => {
      const path = pathOf.get(id);
      return path ? [{ path, text: content }] : [];
    });
  }

  async function refresh(): Promise<void> {
    const source = sourceFor(kind());
    const requested = ++generation;
    setFailed(false);
    if (!source) {
      setPack(null);
      setBusy(false);
      return;
    }
    setBusy(true);
    try {
      const result = await api.promptContextPack(source, liveTexts());
      if (requested === generation) setPack(result);
    } catch {
      if (requested !== generation) return;
      logFailure("the files could not be packed as a prompt");
      setPack(null);
      setFailed(true);
    } finally {
      if (requested === generation) setBusy(false);
    }
  }

  async function loadTags(): Promise<void> {
    try {
      const found = (await api.noteAllTags()).map((entry) => entry.tag);
      setTags(found);
      if (tag() === null || !found.includes(tag()!)) setTag(found[0] ?? null);
    } catch {
      logFailure("the folder's tags could not be listed");
      setTags([]);
    }
  }

  /** Opens the dialog on the file in front: its links when the folder holds
   * it, the whole folder when it does not. */
  async function open(): Promise<void> {
    if (notesStore.root() === null) await notesStore.load();
    const root = notesStore.root();
    if (root === null) {
      showToast("Could not find your folder", "error");
      return;
    }
    const activeId = windowRegistry.getActive()?.tabs.activeTabId() ?? null;
    const active = bufferRegistry.activeTabs().find((doc) => doc.id === activeId);
    const path = active?.source_path ?? null;
    const held = path !== null && isSameOrInside(path, root) && path !== root;
    setFile(held ? path : null);
    setFolder(held ? dirname(path) : root);
    setKind(held ? "linked" : "folder");
    setPack(null);
    setOpen(true);
    void loadTags();
    await refresh();
  }

  function choose(next: PackKind): void {
    if (next === kind()) return;
    setKind(next);
    void refresh();
  }

  function chooseTag(next: string): void {
    setTag(next);
    if (kind() === "tag") void refresh();
  }

  function close(): void {
    generation += 1;
    setOpen(false);
    setPack(null);
    setBusy(false);
    setFailed(false);
  }

  /** Puts the prompt on the clipboard and closes; false when there was none. */
  async function copy(): Promise<boolean> {
    const current = pack();
    if (!current || current.files.length === 0) return false;
    try {
      await clipboardStore.copyText(current.text);
    } catch {
      showToast("Could not copy the files", "error");
      return false;
    }
    const count = current.files.length;
    showToast(`Copied ${count} ${count === 1 ? "file" : "files"} as a prompt`, "success");
    close();
    return true;
  }

  return {
    isOpen,
    kind,
    file,
    folder,
    tags,
    tag,
    pack,
    busy,
    failed,
    open,
    choose,
    chooseTag,
    close,
    copy,
  };
}

// Singleton state — Writ is single-window: one pack dialog.
export const contextPackStore = createRoot(createContextPackStore);
