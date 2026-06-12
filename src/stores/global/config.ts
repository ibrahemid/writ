import { createSignal, createRoot } from "solid-js";
import type { WritConfig, CommandUsage } from "../../types/config";
import * as api from "../../services/tauri";

// Singleton — app-global, not window-scoped (ADR-009 E3).
// Config is shared by every window; mutations persist to disk for all.

const DEFAULT_CONFIG: WritConfig = {
  hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
  sidebar: { toggle: "CmdOrCtrl+S", default_visible: false, position: "left", open: false },
  editor: { font_family: "monospace", font_size: 14, word_wrap: true, tab_size: 2, autosave_debounce_ms: 300, markdown_typography: true },
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
};

const USAGE_FLUSH_DEBOUNCE_MS = 750;

function normalizeIncomingConfig(incoming: WritConfig): WritConfig {
  return {
    ...incoming,
    commands: {
      usage: incoming.commands?.usage ?? {},
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
    scheduleUsageFlush();
  }

  function scheduleUsageFlush() {
    if (flushTimer) clearTimeout(flushTimer);
    flushTimer = setTimeout(() => {
      flushTimer = null;
      void api.updateConfig(config()).catch((err) => {
        console.error("[configStore] failed to flush command usage", err);
      });
    }, USAGE_FLUSH_DEBOUNCE_MS);
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
    scheduleUsageFlush();
  }

  return {
    config,
    load,
    save,
    recordCommandUse,
    clearCommandUsage,
    pruneCommandUsage,
  };
}

export const configStore = createRoot(createConfigStore);
