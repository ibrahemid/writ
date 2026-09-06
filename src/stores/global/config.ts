import { createSignal, createRoot } from "solid-js";
import type { AppearanceConfig, WritConfig, CommandUsage } from "../../types/config";
import * as api from "../../services/tauri";
import { showToast } from "../../components/Notifications/Toast";
import { logFailure } from "../../lib/log";

// Singleton — app-global, not window-scoped (ADR-009 E3).
// Config is shared by every window; mutations persist to disk for all.

// Editor font bounds. The single source of truth for both the Settings input
// and the editor zoom commands — neither hardcodes its own range.
export const EDITOR_FONT_MIN = 8;
export const EDITOR_FONT_MAX = 72;
export const EDITOR_FONT_DEFAULT = 16;

export function clampEditorFontSize(size: number): number {
  if (!Number.isFinite(size)) return EDITOR_FONT_DEFAULT;
  return Math.min(EDITOR_FONT_MAX, Math.max(EDITOR_FONT_MIN, Math.round(size)));
}

// Sidebar width bounds (ADR-030 decision 3). The single source of truth for
// the drag handle, its keyboard steps and the persisted value.
export const SIDEBAR_WIDTH_MIN = 200;
export const SIDEBAR_WIDTH_MAX = 320;
export const SIDEBAR_WIDTH_DEFAULT = 240;

export function clampSidebarWidth(width: number): number {
  if (!Number.isFinite(width)) return SIDEBAR_WIDTH_DEFAULT;
  return Math.min(SIDEBAR_WIDTH_MAX, Math.max(SIDEBAR_WIDTH_MIN, Math.round(width)));
}

// Panel width bounds. The panel takes the sidebar's numbers so the two edges
// of the window match, and its own constants so a later change to one bound
// cannot move the other edge by accident.
export const PANEL_WIDTH_MIN = 200;
export const PANEL_WIDTH_MAX = 320;
export const PANEL_WIDTH_DEFAULT = 240;

export function clampPanelWidth(width: number): number {
  if (!Number.isFinite(width)) return PANEL_WIDTH_DEFAULT;
  return Math.min(PANEL_WIDTH_MAX, Math.max(PANEL_WIDTH_MIN, Math.round(width)));
}

const DEFAULT_APPEARANCE: AppearanceConfig = {
  polarity: "system",
  accent: "pine",
  prose_face: "system",
};

const DEFAULT_CONFIG: WritConfig = {
  hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
  sidebar: {
    toggle: "CmdOrCtrl+\\",
    default_visible: false,
    position: "left",
    open: true,
    width: SIDEBAR_WIDTH_DEFAULT,
  },
  // Closed on a first launch: the window opens on a cursor and nothing else.
  panel: { open: false, width: PANEL_WIDTH_DEFAULT },
  first_run: { hint_dismissed: false },
  editor: { font_family: "monospace", font_size: EDITOR_FONT_DEFAULT, word_wrap: true, tab_size: 2, autosave_debounce_ms: 1000, markdown_typography: true, markdown_editing: true, status_bar: false },
  window: { width: 1100, height: 720, maximized: false },
  keybindings: {},
  history: { max_entries: 500 },
  storage: { path: "~/.writ" },
  theme: { preset: "writ-light", overrides: {} },
  appearance: DEFAULT_APPEARANCE,
  commands: { usage: {} },
  preview: {
    default_layout_html: "split",
    default_layout_markdown: "split",
    live_render_threshold_mb: 1,
    render_confirm_threshold_mb: 5,
    render_refuse_threshold_mb: 50,
    debounce_ms: 200,
    run_scripts: true,
  },
  workspace: { root: null },
  inbox: { path: null, focus: true },
  updater: { auto_check: true },
  ai: {
    enabled: false,
    preset: "ollama",
    base_url: "http://localhost:11434/v1",
    model: "",
    consented_hosts: [],
  },
  spelling: { enabled: false, dialect: "american", ignored_words: [] },
};

const PERSIST_DEBOUNCE_MS = 750;

function normalizeIncomingConfig(incoming: WritConfig): WritConfig {
  return {
    ...incoming,
    sidebar: {
      ...incoming.sidebar,
      width: clampSidebarWidth(incoming.sidebar?.width ?? SIDEBAR_WIDTH_DEFAULT),
    },
    panel: {
      open: incoming.panel?.open ?? false,
      width: clampPanelWidth(incoming.panel?.width ?? PANEL_WIDTH_DEFAULT),
    },
    first_run: {
      hint_dismissed: incoming.first_run?.hint_dismissed ?? false,
    },
    commands: {
      usage: incoming.commands?.usage ?? {},
    },
    workspace: { root: incoming.workspace?.root ?? null },
    inbox: {
      path: incoming.inbox?.path ?? null,
      focus: incoming.inbox?.focus ?? true,
    },
    updater: {
      auto_check: incoming.updater?.auto_check ?? true,
    },
    appearance: {
      polarity: incoming.appearance?.polarity ?? DEFAULT_APPEARANCE.polarity,
      accent: incoming.appearance?.accent ?? DEFAULT_APPEARANCE.accent,
      prose_face: incoming.appearance?.prose_face ?? DEFAULT_APPEARANCE.prose_face,
    },
    ai: {
      enabled: incoming.ai?.enabled ?? false,
      preset: incoming.ai?.preset ?? "ollama",
      base_url: incoming.ai?.base_url ?? "http://localhost:11434/v1",
      model: incoming.ai?.model ?? "",
      consented_hosts: incoming.ai?.consented_hosts ?? [],
    },
    spelling: {
      enabled: incoming.spelling?.enabled ?? false,
      dialect: incoming.spelling?.dialect ?? "american",
      ignored_words: incoming.spelling?.ignored_words ?? [],
    },
  };
}

function pruneUsage(
  usage: Record<string, CommandUsage>,
  knownIds: ReadonlySet<string>,
): Record<string, CommandUsage> {
  const next: Record<string, CommandUsage> = {};
  let changed = false;
  for (const [id, entry] of Object.entries(usage)) {
    if (knownIds.has(id)) {
      next[id] = entry;
    } else {
      changed = true;
    }
  }
  return changed ? next : usage;
}

function createConfigStore() {
  const [config, setConfig] = createSignal<WritConfig>(DEFAULT_CONFIG);
  let flushTimer: ReturnType<typeof setTimeout> | null = null;
  // Config reloads on every `config:changed` event and persists on a debounce,
  // so a lasting failure would toast on repeat. Each is reported once and
  // re-armed by the next success.
  let loadFailureReported = false;
  let persistFailureReported = false;

  async function load() {
    try {
      const loaded = await api.getConfig();
      setConfig(normalizeIncomingConfig(loaded));
      loadFailureReported = false;
    } catch {
      setConfig(DEFAULT_CONFIG);
      if (loadFailureReported) return;
      loadFailureReported = true;
      logFailure("settings could not be read");
      showToast(
        "Couldn't read your settings. Writ is using defaults; saving now replaces the stored file.",
        "error",
        8000,
      );
    }
  }

  async function save(updated: WritConfig) {
    const normalized = normalizeIncomingConfig(updated);
    await api.updateConfig(normalized);
    setConfig(normalized);
  }

  function recordCommandUse(id: string, nowMs: number = Date.now()) {
    const current = config();
    const prev = current.commands.usage[id];
    const next: CommandUsage = {
      count: (prev?.count ?? 0) + 1,
      last_used_ms: nowMs,
    };
    setConfig({
      ...current,
      commands: {
        ...current.commands,
        usage: { ...current.commands.usage, [id]: next },
      },
    });
    schedulePersist();
  }

  // Optimistically apply a new editor font size and persist it through the
  // same config layer the Settings input uses (single source of truth). The
  // write is debounced so a fast zoom (Cmd+scroll, key repeat) coalesces into
  // one disk write while the editor reflows instantly off the live signal.
  function setEditorFontSize(size: number) {
    const clamped = clampEditorFontSize(size);
    const current = config();
    if (current.editor.font_size === clamped) return;
    setConfig({
      ...current,
      editor: { ...current.editor, font_size: clamped },
    });
    schedulePersist();
  }

  // The drag handle commits on release, the keyboard on each step; both go
  // through the same debounce as every other setting.
  function setSidebarWidth(width: number) {
    const clamped = clampSidebarWidth(width);
    const current = config();
    if (current.sidebar.width === clamped) return;
    setConfig({
      ...current,
      sidebar: { ...current.sidebar, width: clamped },
    });
    schedulePersist();
  }

  // The panel's toggle writes on each flip and its drag handle on release;
  // both go through the same debounce as every other setting.
  function setPanelOpen(open: boolean) {
    const current = config();
    if (current.panel.open === open) return;
    setConfig({ ...current, panel: { ...current.panel, open } });
    schedulePersist();
  }

  function setPanelWidth(width: number) {
    const clamped = clampPanelWidth(width);
    const current = config();
    if (current.panel.width === clamped) return;
    setConfig({ ...current, panel: { ...current.panel, width: clamped } });
    schedulePersist();
  }

  function schedulePersist() {
    if (flushTimer) clearTimeout(flushTimer);
    flushTimer = setTimeout(() => {
      flushTimer = null;
      void api.updateConfig(config()).then(
        () => {
          persistFailureReported = false;
        },
        () => {
          if (persistFailureReported) return;
          persistFailureReported = true;
          logFailure("settings could not be written");
          showToast("Couldn't save your settings. The change applies to this session only.", "error");
        },
      );
    }, PERSIST_DEBOUNCE_MS);
  }

  async function clearCommandUsage() {
    const current = config();
    const updated: WritConfig = {
      ...current,
      commands: { ...current.commands, usage: {} },
    };
    await save(updated);
  }

  // Rust has already written this one; the copy here catches up so the next
  // whole-config write does not carry the old answer back over it. Writ's own
  // config write is stamped into the watcher's ignore set, so nothing else
  // tells this store the file moved.
  function noteFirstRunHintDismissed() {
    const current = config();
    if (current.first_run.hint_dismissed) return;
    setConfig({ ...current, first_run: { ...current.first_run, hint_dismissed: true } });
  }

  function pruneCommandUsage(knownIds: ReadonlySet<string>) {
    const current = config();
    const pruned = pruneUsage(current.commands.usage, knownIds);
    if (pruned === current.commands.usage) return;
    const updated: WritConfig = {
      ...current,
      commands: { ...current.commands, usage: pruned },
    };
    setConfig(updated);
    schedulePersist();
  }

  return {
    config,
    load,
    save,
    recordCommandUse,
    setEditorFontSize,
    setSidebarWidth,
    setPanelOpen,
    setPanelWidth,
    clearCommandUsage,
    pruneCommandUsage,
    noteFirstRunHintDismissed,
  };
}

export const configStore = createRoot(createConfigStore);
