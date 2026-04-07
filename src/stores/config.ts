import { createSignal, createRoot } from "solid-js";
import type { WritConfig } from "../types/config";
import * as api from "../services/tauri";

const DEFAULT_CONFIG: WritConfig = {
  hotkey: { toggle: "CmdOrCtrl+Shift+Space" },
  sidebar: { toggle: "CmdOrCtrl+B", default_visible: false, position: "left" },
  editor: { font_family: "monospace", font_size: 14, word_wrap: true, tab_size: 2, autosave_debounce_ms: 300 },
  window: { width: 800, height: 600 },
  keybindings: {},
  history: { max_entries: 500 },
  storage: { path: "~/.writ" },
};

function createConfigStore() {
  const [config, setConfig] = createSignal<WritConfig>(DEFAULT_CONFIG);

  async function load() {
    try {
      const loaded = await api.getConfig();
      setConfig(loaded);
    } catch (e) {
      console.error("failed to load config:", e);
    }
  }

  async function save(updated: WritConfig) {
    await api.updateConfig(updated);
    setConfig(updated);
  }

  return { config, load, save };
}

export const configStore = createRoot(createConfigStore);
