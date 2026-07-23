import { createSignal, createRoot } from "solid-js";
import type { WritConfig, CommandUsage } from "../../types/config";
import * as api from "../../services/tauri";

// Singleton — app-global, not window-scoped (ADR-009 E3).
// Config is shared by every window; mutations persist to disk for all.

// Editor font bounds. The single source of truth for both the Settings input
// and the editor zoom commands — neither hardcodes its own range.
export const EDITOR_FONT_MIN = 8;
export const EDITOR_FONT_MAX = 72;
export const EDITOR_FONT_DEFAULT = 14;

export function clampEditorFontSize(size: number): number {
  if (!Number.isFinite(size)) return EDITOR_FONT_DEFAULT;
  return Math.min(EDITOR_FONT_MAX, Math.max(EDITOR_FONT_MIN, Math.round(size)));
}

const DEFAULT_CONFIG: WritConfig = {
  hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
  sidebar: { toggle: "CmdOrCtrl+S", default_visible: false, position: "left", open: false },
  editor: { font_family: "monospace", font_size: EDITOR_FONT_DEFAULT, word_wrap: true, tab_size: 2, autosave_debounce_ms: 300, markdown_typography: true, markdown_editing: true },
  window: { width: 1100, height: 720 },
  keybindings: {},
  history: { max_entries: 500 },
  storage: { path: "~/.writ" },
  theme: { preset: "warp-dark", overrides: {} },
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
};

const PERSIST_DEBOUNCE_MS = 750;

function normalizeIncomingConfig(incoming: WritConfig): WritConfig {
  return {
    ...incoming,
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
    ai: {
      enabled: incoming.ai?.enabled ?? false,
      preset: incoming.ai?.preset ?? "ollama",
      base_url: incoming.ai?.base_url ?? "http://localhost:11434/v1",
      model: incoming.ai?.model ?? "",
      consented_hosts: incoming.ai?.consented_hosts ?? [],
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

  async function load() {
    try {
      const loaded = await api.getConfig();
      setConfig(normalizeIncomingConfig(loaded));
    } catch (err) {
      console.error("[configStore] failed to load config", err);
      setConfig(DEFAULT_CONFIG);
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

  function schedulePersist() {
    if (flushTimer) clearTimeout(flushTimer);
    flushTimer = setTimeout(() => {
      flushTimer = null;
      void api.updateConfig(config()).catch((err) => {
        console.error("[configStore] failed to persist config", err);
      });
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
    clearCommandUsage,
    pruneCommandUsage,
  };
}

export const configStore = createRoot(createConfigStore);
