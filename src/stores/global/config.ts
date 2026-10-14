import { batch, createRoot, createSignal } from "solid-js";
import { createStore, reconcile, unwrap } from "solid-js/store";
import type {
  AppId,
  AiConfig,
  AppearanceConfig,
  ConfigDiagnostic,
  FileExtension,
  SidebarSectionId,
  WritConfig,
} from "../../types/config";
import * as api from "../../services/tauri";
import { dismissToast, showToast } from "./notifications";
import { logFailure } from "../../lib/log";
import { knownSidebarSections, withSidebarSection } from "../../lib/sidebar-sections";
import { SIDEBAR } from "../../styles/generated/tokens";

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
// the drag handle, its keyboard steps and the persisted value, read from the
// tokens the stylesheet lays the panel out with.
export const SIDEBAR_WIDTH_MIN = Number.parseFloat(SIDEBAR.minWidth);
export const SIDEBAR_WIDTH_MAX = Number.parseFloat(SIDEBAR.maxWidth);
export const SIDEBAR_WIDTH_DEFAULT = Number.parseFloat(SIDEBAR.width);

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

// Chat column bounds. Wider than the panel's range because the column holds a
// conversation and, while one is open, a proposal beside the note it changes.
export const CHAT_WIDTH_MIN = 320;
export const CHAT_WIDTH_MAX = 520;
export const CHAT_WIDTH_DEFAULT = 380;

export function clampChatWidth(width: number): number {
  if (!Number.isFinite(width)) return CHAT_WIDTH_DEFAULT;
  return Math.min(CHAT_WIDTH_MAX, Math.max(CHAT_WIDTH_MIN, Math.round(width)));
}

// Interface text bounds (spec A1). The settings row and the root token both
// read these; null keeps whatever size the platform layer already resolves.
export const INTERFACE_TEXT_MIN = 12;
export const INTERFACE_TEXT_MAX = 22;

export function clampInterfaceTextSize(size: number): number | null {
  if (!Number.isFinite(size)) return null;
  return Math.min(INTERFACE_TEXT_MAX, Math.max(INTERFACE_TEXT_MIN, Math.round(size)));
}

const DEFAULT_APPEARANCE: AppearanceConfig = {
  polarity: "system",
  accent: "pine",
  prose_face: "system",
  interface_text_size: null,
};

const DEFAULT_CONFIG: WritConfig = {
  hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
  sidebar: {
    toggle: "CmdOrCtrl+\\",
    default_visible: false,
    position: "left",
    open: true,
    width: SIDEBAR_WIDTH_DEFAULT,
    collapsed: [],
    // Rust's `WritConfig::default`: the file tree and search (ADR-042).
    hidden: ["recent"],
  },
  // Closed on a first launch: the window opens on a cursor and nothing else.
  panel: { open: false, width: PANEL_WIDTH_DEFAULT },
  chat_panel: { open: false, width: CHAT_WIDTH_DEFAULT },
  first_run: { hint_dismissed: false },
  editor: { font_size: EDITOR_FONT_DEFAULT, word_wrap: true, tab_size: 2, autosave_debounce_ms: 1000, markdown_editing: true, status_bar: true, status_bar_counts: false },
  window: { width: 1100, height: 720, maximized: false },
  keybindings: {},
  history: { max_entries: 500 },
  theme: { preset: "writ-light", overrides: {} },
  appearance: DEFAULT_APPEARANCE,
  files: { default_extension: "txt" },
  preview: {
    default_layout_html: "split",
    default_layout_markdown: "inline",
    live_render_threshold_mb: 1,
    render_confirm_threshold_mb: 5,
    render_refuse_threshold_mb: 50,
    debounce_ms: 200,
  },
  workspace: { root: null },
  inbox: { path: null, focus: true },
  updater: { auto_check: true },
  ai: {
    provider: "ollama",
    base_url: "",
    model: "",
    consented_hosts: [],
    rewrite: { enabled: false },
    chat: { enabled: false, model: "", model_provider: "" },
  },
  mcp: { enabled: false, approved_clients: [] },
  spelling: { enabled: false, dialect: "american", ignored_words: [] },
  apps: { connections: false, graph: false, tags: false },
};

const PERSIST_DEBOUNCE_MS = 750;

/** A plain copy of a config, sharing nothing with the store. The store adopts
 * and mutates the objects it is given, and the config is JSON-shaped because
 * it arrives over IPC. */
function plainCopy(config: WritConfig): WritConfig {
  return JSON.parse(JSON.stringify(unwrap(config))) as WritConfig;
}

// A config written before Writ had a format to ask about carries no [files]
// table, and one written by hand can carry a word that is neither extension.
// Both answer the same way the first launch answers when nobody has chosen.
function knownExtension(value: unknown): FileExtension {
  return value === "md" || value === "txt" ? value : DEFAULT_CONFIG.files.default_extension;
}

/** Whether `app` is on in `config`. */
export function isAppOnIn(config: WritConfig, app: AppId): boolean {
  switch (app) {
    case "chat":
      return config.ai.chat.enabled;
    case "rewrite":
      return config.ai.rewrite.enabled;
    case "programs":
      return config.mcp.enabled;
    case "connections":
      return config.apps.connections;
    case "graph":
      return config.apps.graph;
    case "tags":
      return config.apps.tags;
  }
}

/** `config` with `app` switched, on the key that app is read from. */
export function withApp(config: WritConfig, app: AppId, on: boolean): WritConfig {
  switch (app) {
    case "chat":
      return { ...config, ai: { ...config.ai, chat: { ...config.ai.chat, enabled: on } } };
    case "rewrite":
      return { ...config, ai: { ...config.ai, rewrite: { ...config.ai.rewrite, enabled: on } } };
    case "programs":
      return { ...config, mcp: { ...config.mcp, enabled: on } };
    case "connections":
    case "graph":
    case "tags":
      return { ...config, apps: { ...config.apps, [app]: on } };
  }
}

function normalizeIncomingConfig(incoming: WritConfig): WritConfig {
  return {
    ...incoming,
    sidebar: {
      ...incoming.sidebar,
      width: clampSidebarWidth(incoming.sidebar?.width ?? SIDEBAR_WIDTH_DEFAULT),
      collapsed: knownSidebarSections(incoming.sidebar?.collapsed),
      hidden: knownSidebarSections(incoming.sidebar?.hidden),
    },
    panel: {
      open: incoming.panel?.open ?? false,
      width: clampPanelWidth(incoming.panel?.width ?? PANEL_WIDTH_DEFAULT),
    },
    chat_panel: {
      open: incoming.chat_panel?.open ?? false,
      width: clampChatWidth(incoming.chat_panel?.width ?? CHAT_WIDTH_DEFAULT),
    },
    first_run: {
      hint_dismissed: incoming.first_run?.hint_dismissed ?? false,
    },
    files: {
      default_extension: knownExtension(incoming.files?.default_extension),
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
      interface_text_size: incoming.appearance?.interface_text_size ?? null,
    },
    // The new shape only. An older `[ai]` table is migrated once, in Rust
    // (`AiConfigOnDisk`), so a second copy of those rules cannot drift from it.
    ai: {
      provider: incoming.ai?.provider ?? "ollama",
      base_url: incoming.ai?.base_url ?? "",
      model: incoming.ai?.model ?? "",
      consented_hosts: incoming.ai?.consented_hosts ?? [],
      rewrite: { enabled: incoming.ai?.rewrite?.enabled ?? false },
      chat: {
        enabled: incoming.ai?.chat?.enabled ?? false,
        model: incoming.ai?.chat?.model ?? "",
        model_provider: incoming.ai?.chat?.model_provider ?? "",
      },
    },
    mcp: {
      enabled: incoming.mcp?.enabled ?? false,
      approved_clients: incoming.mcp?.approved_clients ?? [],
    },
    apps: {
      connections: incoming.apps?.connections ?? false,
      graph: incoming.apps?.graph ?? false,
      tags: incoming.apps?.tags ?? false,
    },
    spelling: {
      enabled: incoming.spelling?.enabled ?? false,
      dialect: incoming.spelling?.dialect ?? "american",
      ignored_words: incoming.spelling?.ignored_words ?? [],
    },
  };
}

/** What the notice says about the values in config.toml the launch could not
 * use, or `null` when there are none. Names the first by key and line. */
export function unreadValuesNotice(unread: readonly ConfigDiagnostic[]): string | null {
  const [first] = unread;
  if (!first) return null;
  const tail = "Settings won't be saved until the file is fixed.";
  if (!first.key) {
    return `Writ could not read config.toml (line ${first.line}) and is using default settings. ${tail}`;
  }
  const others = unread.length - 1;
  if (others === 0) {
    return `Writ could not read ${first.key} on line ${first.line} of config.toml and is using its default. ${tail}`;
  }
  const more = others === 1 ? "1 other value" : `${others} other values`;
  return `Writ could not read ${first.key} on line ${first.line} of config.toml and ${more}, and is using defaults for them. ${tail}`;
}

/**
 * The settings, as a store read field by field (ADR-048).
 *
 * `config()` answers the store itself, so an effect that reads
 * `config().editor.font_size` reruns when that field changes and for nothing
 * else. Every write goes through a section setter or through `reconcile`,
 * which changes only the fields whose value differs: a reload of a file that
 * changed one value, or a whole-config save from Settings, reruns the readers
 * of that value alone.
 */
function createConfigStore() {
  const [state, setState] = createStore<WritConfig>(plainCopy(DEFAULT_CONFIG));
  // Whether the first load has answered, read or failed. Window state derived
  // from the config waits for it, so a launch does not show the defaults'
  // layout before the stored one.
  const [loaded, setLoaded] = createSignal(false);
  let flushTimer: ReturnType<typeof setTimeout> | null = null;
  // Config reloads on every `config:changed` event and persists on a debounce,
  // so a lasting failure would toast on repeat. Each is reported once and
  // re-armed by the next success.
  let loadFailureReported = false;
  let persistFailureReported = false;
  let unreadNotice: number | null = null;

  const config = (): WritConfig => state;

  /** Shows the notice naming the values config.toml holds that the launch
   * could not use, and takes it down once a reload reads the file in full. */
  async function noticeUnreadValues(): Promise<void> {
    let unread: ConfigDiagnostic[];
    try {
      unread = await api.getConfigDiagnostics();
    } catch {
      logFailure("unread settings could not be listed");
      return;
    }
    const text = unreadValuesNotice(unread);
    if (text === null) {
      if (unreadNotice !== null) dismissToast(unreadNotice);
      unreadNotice = null;
      return;
    }
    if (unreadNotice === null) unreadNotice = showToast(text, "warning", 0);
  }

  function replaceWith(next: WritConfig) {
    setState(reconcile(next));
  }

  async function load(): Promise<boolean> {
    try {
      const loaded = await api.getConfig();
      replaceWith(normalizeIncomingConfig(loaded));
      loadFailureReported = false;
      return true;
    } catch {
      replaceWith(plainCopy(DEFAULT_CONFIG));
      if (loadFailureReported) return false;
      loadFailureReported = true;
      logFailure("settings could not be read");
      showToast(
        "Could not read your settings. Writ is using defaults; saving now replaces the stored file.",
        "error",
        8000,
      );
      return false;
    } finally {
      setLoaded(true);
    }
  }

  async function save(updated: WritConfig) {
    const normalized = plainCopy(normalizeIncomingConfig(updated));
    await api.updateConfig(normalized);
    replaceWith(normalized);
  }

  /** Puts a connection Rust has already written into the running config.
   *
   * `ai_set_provider` saves the file itself and answers what it saved, so the
   * surface that asked for the change applies that answer rather than reading
   * the file back and racing whatever else is writing it. */
  function applyAi(ai: AiConfig) {
    setState("ai", reconcile(ai));
  }

  // Optimistically apply a new editor font size and persist it through the
  // same config layer the Settings input uses (single source of truth). The
  // write is debounced so a fast zoom (Cmd+scroll, key repeat) coalesces into
  // one disk write while the editor reflows instantly off the store.
  function setEditorFontSize(size: number) {
    const clamped = clampEditorFontSize(size);
    if (state.editor.font_size === clamped) return;
    setState("editor", "font_size", clamped);
    schedulePersist();
  }

  // The drag handle commits on release, the keyboard on each step; both go
  // through the same debounce as every other setting.
  function setSidebarWidth(width: number) {
    const clamped = clampSidebarWidth(width);
    if (state.sidebar.width === clamped) return;
    setState("sidebar", "width", clamped);
    schedulePersist();
  }

  /** Shows or hides the sidebar. The window reads its open state from here,
   * so the change shows at once and the file follows. */
  function setSidebarOpen(open: boolean) {
    if (state.sidebar.open === open) return;
    setState("sidebar", "open", open);
    persistNow();
  }

  function isSidebarSectionCollapsed(id: SidebarSectionId): boolean {
    return state.sidebar.collapsed.includes(id);
  }

  function isSidebarSectionHidden(id: SidebarSectionId): boolean {
    return state.sidebar.hidden.includes(id);
  }

  // A fold is restored on the next launch, so it is written like the width:
  // once per flip, on the shared debounce.
  function setSidebarSectionCollapsed(id: SidebarSectionId, collapsed: boolean) {
    const current = state.sidebar.collapsed;
    const next = withSidebarSection(current, id, collapsed);
    if (next === current) return;
    setState("sidebar", "collapsed", [...next]);
    schedulePersist();
  }

  function setSidebarSectionHidden(id: SidebarSectionId, hidden: boolean) {
    const current = state.sidebar.hidden;
    const next = withSidebarSection(current, id, hidden);
    if (next === current) return;
    setState("sidebar", "hidden", [...next]);
    schedulePersist();
  }

  // The panel's toggle writes on each flip and its drag handle on release;
  // both go through the same debounce as every other setting.
  function setPanelOpen(open: boolean) {
    if (state.panel.open === open) return;
    setState("panel", "open", open);
    schedulePersist();
  }

  function setPanelWidth(width: number) {
    const clamped = clampPanelWidth(width);
    if (state.panel.width === clamped) return;
    setState("panel", "width", clamped);
    schedulePersist();
  }

  // The chat column remembers what the panel beside the note remembers, in
  // the same file and on the same debounce.
  function setChatPanelOpen(open: boolean) {
    if (state.chat_panel.open === open) return;
    setState("chat_panel", "open", open);
    schedulePersist();
  }

  function setChatPanelWidth(width: number) {
    const clamped = clampChatWidth(width);
    if (state.chat_panel.width === clamped) return;
    setState("chat_panel", "width", clamped);
    schedulePersist();
  }

  function writeCurrent() {
    void api.updateConfig(plainCopy(state)).then(
      () => {
        persistFailureReported = false;
      },
      () => {
        if (persistFailureReported) return;
        persistFailureReported = true;
        logFailure("settings could not be written");
        showToast("Could not save your settings. The change applies to this session only.", "error");
      },
    );
  }

  function schedulePersist() {
    if (flushTimer) clearTimeout(flushTimer);
    flushTimer = setTimeout(() => {
      flushTimer = null;
      writeCurrent();
    }, PERSIST_DEBOUNCE_MS);
  }

  /** Writes at once, taking any debounced change with it. */
  function persistNow() {
    if (flushTimer) {
      clearTimeout(flushTimer);
      flushTimer = null;
    }
    writeCurrent();
  }

  // Rust has already written this one; the copy here catches up so the next
  // whole-config write does not carry the old answer back over it. Writ's own
  // config write is stamped into the watcher's ignore set, so nothing else
  // tells this store the file moved.
  function noteFirstRunHintDismissed() {
    if (state.first_run.hint_dismissed) return;
    setState("first_run", "hint_dismissed", true);
  }

  /** Whether `app` is on, read from the key that app has always used
   * (`WritConfig::is_app_on` in Rust). Reactive. */
  function isAppOn(app: AppId): boolean {
    return isAppOnIn(state, app);
  }

  /** Turns an app on or off and writes the config at once, so the menu bar
   * Rust draws from the file follows the switch. */
  async function setAppOn(app: AppId, on: boolean): Promise<void> {
    if (isAppOnIn(state, app) === on) return;
    await save(withApp(state, app, on));
  }

  function switchApp(app: AppId, on: boolean) {
    switch (app) {
      case "chat":
        setState("ai", "chat", "enabled", on);
        return;
      case "rewrite":
        setState("ai", "rewrite", "enabled", on);
        return;
      case "programs":
        setState("mcp", "enabled", on);
        return;
      case "connections":
      case "graph":
      case "tags":
        setState("apps", app, on);
        return;
    }
  }

  /** Puts apps Rust has already switched on (the first launch) into the
   * running copy, so the next whole-config write keeps them. */
  function noteAppsOn(apps: readonly AppId[]) {
    batch(() => {
      for (const app of apps) switchApp(app, true);
    });
  }

  // The same catching-up as `noteFirstRunHintDismissed`: `finish_first_run`
  // wrote the chosen format to disk with the rest of the config, and this copy
  // follows so the next whole-config write does not carry the old answer back.
  function noteDefaultExtension(extension: FileExtension) {
    if (state.files.default_extension === extension) return;
    setState("files", "default_extension", extension);
  }

  return {
    config,
    loaded,
    load,
    noticeUnreadValues,
    save,
    applyAi,
    setEditorFontSize,
    setSidebarWidth,
    setSidebarOpen,
    isSidebarSectionCollapsed,
    isSidebarSectionHidden,
    setSidebarSectionCollapsed,
    setSidebarSectionHidden,
    setPanelOpen,
    setPanelWidth,
    setChatPanelOpen,
    setChatPanelWidth,
    noteFirstRunHintDismissed,
    noteDefaultExtension,
    isAppOn,
    setAppOn,
    noteAppsOn,
  };
}

export const configStore = createRoot(createConfigStore);
