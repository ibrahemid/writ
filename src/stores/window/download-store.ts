import { createSignal } from "solid-js";
import * as api from "../../services/tauri";
import { onEvent, type UnlistenFn } from "../../services/events";

export type DownloadStore = ReturnType<typeof createDownloadStore>;

// How far a note's download has got, from this window's point of view. A
// download that is still running is "downloading"; the other three are the
// ways it ended without the bytes arriving, and each keeps its entry until the
// person closes it.
export type DownloadState = "downloading" | "failed" | "timed_out";

// Which part of getting the note here gave out. The provider's own failures
// are the "download" ones; the other two are Writ's, and each needs its own
// sentence because "this file could not be downloaded" would be untrue.
export type DownloadFailure = "download" | "open" | "listener";

export interface PendingDownload {
  // Canonical path of the file being downloaded. Identity for the entry.
  path: string;
  // The note's name, as the tab shows it.
  title: string;
  // The sync provider's name as the user knows it, or null when it is unknown.
  provider: string | null;
  state: DownloadState;
  // What went wrong, once the state is "failed".
  reason: DownloadFailure;
  // What the provider said went wrong, when it said anything.
  message: string | null;
}

// Per-window state for notes whose bytes are not on this machine yet. A file
// like that opens no buffer, so it cannot live in the buffer registry: it has
// a tab and a pane of its own until it either arrives or stops.
export function createDownloadStore() {
  const [pending, setPending] = createSignal<PendingDownload[]>([]);
  const [selectedPath, setSelectedPath] = createSignal<string | null>(null);
  // Whether the note-download listener attached, once mount has been called.
  // Never rejects: a download that nothing is listening for has to be said
  // rather than thrown, or the tab waits for a state that cannot arrive.
  let listening: Promise<boolean> | null = null;
  let reopen: (path: string, options?: { activate?: boolean }) => Promise<unknown> = async () =>
    undefined;

  // The store asks for the note again once the bytes land, and opening a note
  // is the tab store's job. Attached after both stores exist rather than
  // injected, because each needs the other.
  function attachOpener(
    open: (path: string, options?: { activate?: boolean }) => Promise<unknown>,
  ): void {
    reopen = open;
  }

  function find(path: string): PendingDownload | undefined {
    return pending().find((d) => d.path === path);
  }

  function selected(): PendingDownload | null {
    const path = selectedPath();
    if (!path) return null;
    return find(path) ?? null;
  }

  function select(path: string | null): void {
    setSelectedPath(path);
  }

  function update(path: string, change: Partial<PendingDownload>): void {
    setPending((prev) => prev.map((d) => (d.path === path ? { ...d, ...change } : d)));
  }

  // Puts an entry the store had already dropped back, carrying why. Skipped
  // when something has taken the path over in the meantime, which is what a
  // note that came back a placeholder does.
  function restoreAsFailed(entry: PendingDownload, reason: DownloadFailure, select: boolean): void {
    setPending((prev) =>
      prev.some((d) => d.path === entry.path)
        ? prev
        : [...prev, { ...entry, state: "failed", reason, message: null }],
    );
    if (select) setSelectedPath(entry.path);
  }

  // Gives the one-shot permission to open this note back to Rust. A note that
  // is not downloaded is authorized once, for the open that follows the bytes
  // landing; when that open never happens, nothing else spends it.
  async function handBackOpenPermission(path: string): Promise<void> {
    await api.cancelMaterialiseNote(path).catch(() => undefined);
  }

  function drop(path: string): void {
    setPending((prev) => prev.filter((d) => d.path !== path));
    if (selectedPath() === path) setSelectedPath(null);
  }

  // Adds the note to the pending list and asks for its bytes. A path already
  // pending is selected rather than added twice: the download it is waiting on
  // is the same one a second open would ask for.
  async function start(entry: { path: string; title: string; provider: string | null }): Promise<void> {
    const existing = find(entry.path);
    setSelectedPath(entry.path);
    if (existing) {
      if (existing.state === "downloading") return;
      update(entry.path, { state: "downloading", reason: "download", message: null });
    } else {
      setPending((prev) => [
        ...prev,
        { ...entry, state: "downloading", reason: "download", message: null },
      ]);
    }
    // Asked for only once the listener is attached: the download reports every
    // state it passes through as an event, so one that starts before Writ is
    // listening can finish unheard and leave the tab downloading for good.
    if (listening !== null && !(await listening)) {
      update(entry.path, { state: "failed", reason: "listener", message: null });
      await handBackOpenPermission(entry.path);
      return;
    }
    try {
      await api.materialiseNote(entry.path);
    } catch (error) {
      // The download never started, so no event will ever arrive for it. Say
      // so here or the entry waits for the rest of the session.
      update(entry.path, { state: "failed", reason: "download", message: String(error) });
    }
  }

  // Stops waiting. The entry goes with the wait: there is nothing left to show
  // about a download the person called off, and nothing was read.
  async function cancel(path: string): Promise<void> {
    drop(path);
    // Best effort: the flag stops Writ waiting, and a call that does not land
    // leaves a wait that times out on its own with nothing open behind it.
    await api.cancelMaterialiseNote(path).catch(() => undefined);
  }

  // Closes an entry that ended without the bytes. Cancel is the same gesture
  // for a download still running; this one has nothing to call off.
  function close(path: string): void {
    drop(path);
  }

  async function handle(payload: {
    path: string;
    state: "started" | "done" | "failed" | "cancelled" | "timed_out";
    message?: string;
  }): Promise<void> {
    if (!find(payload.path)) return;
    switch (payload.state) {
      case "started":
        return;
      case "done": {
        const path = payload.path;
        // A download the person is watching opens in front of them. One that
        // finished behind another note opens without taking the screen.
        const watching = selectedPath() === path;
        const entry = find(path);
        drop(path);
        try {
          await reopen(path, { activate: watching });
        } catch {
          // The bytes arrived and the note still did not open. Keeping the tab
          // is what stops the note the person asked for disappearing without a
          // word; a second open is theirs to ask for.
          if (entry) restoreAsFailed(entry, "open", watching);
          await handBackOpenPermission(path);
        }
        return;
      }
      case "cancelled":
        drop(payload.path);
        return;
      case "failed":
        update(payload.path, {
          state: "failed",
          reason: "download",
          message: payload.message ?? null,
        });
        return;
      case "timed_out":
        update(payload.path, { state: "timed_out", message: null });
        return;
    }
  }

  async function mount(): Promise<UnlistenFn> {
    const attaching = onEvent("note:download", (payload) => {
      void handle(payload);
    });
    listening = attaching.then(
      () => true,
      () => false,
    );
    return attaching;
  }

  return {
    pending,
    selected,
    selectedPath,
    select,
    attachOpener,
    start,
    cancel,
    close,
    handle,
    mount,
  };
}
