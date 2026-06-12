import { createSignal, createEffect, onCleanup, Show, Switch, Match } from "solid-js";
import { configStore } from "../../stores/global/config";
import { themeStore } from "../../stores/global/theme";
import { PRESETS } from "../../styles/themes";
import { openThemeEditor } from "../ThemeEditor/ThemeEditor";
import { openShortcutEditor } from "../ShortcutEditor/ShortcutEditor";
import { installFocusTrap } from "../../lib/focus-trap";
import { useWindow } from "../WindowProvider/WindowProvider";
import { showToast } from "../Notifications/Toast";
import { installCli } from "../../services/tauri";
import type { DefaultLayout } from "../../types/config";
import "./SettingsModal.css";

type Section = "editor" | "files" | "preview" | "appearance" | "shortcuts";

// Singleton state — Writ is single-window
const [isOpen, setIsOpen] = createSignal(false);
const [activeSection, setActiveSection] = createSignal<Section>("editor");

export function openSettings() {
  setActiveSection("editor");
  setIsOpen(true);
}

export function closeSettings() {
  setIsOpen(false);
}

export function toggleSettings() {
  setIsOpen((prev) => !prev);
}

const NAV_ITEMS: { id: Section; label: string }[] = [
  { id: "editor", label: "Editor" },
  { id: "files", label: "Files" },
  { id: "preview", label: "Preview" },
  { id: "appearance", label: "Appearance" },
  { id: "shortcuts", label: "Shortcuts" },
];

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function parseIntSafe(raw: string, fallback: number): number {
  const n = parseInt(raw, 10);
  return Number.isFinite(n) ? n : fallback;
}

function parseFloatSafe(raw: string, fallback: number): number {
  const n = parseFloat(raw);
  return Number.isFinite(n) ? n : fallback;
}

async function patchConfig(patch: (prev: ReturnType<typeof configStore.config>) => ReturnType<typeof configStore.config>) {
  try {
    await configStore.save(patch(configStore.config()));
  } catch {
    showToast("Failed to save settings", "error");
  }
}

function EditorSection() {
  const cfg = () => configStore.config().editor;

  function onFontSizeChange(raw: string) {
    const value = clamp(parseIntSafe(raw, cfg().font_size), 8, 72);
    void patchConfig((prev) => ({ ...prev, editor: { ...prev.editor, font_size: value } }));
  }

  function onWordWrapToggle() {
    void patchConfig((prev) => ({ ...prev, editor: { ...prev.editor, word_wrap: !prev.editor.word_wrap } }));
  }

  function onTabSizeChange(raw: string) {
    const value = clamp(parseIntSafe(raw, cfg().tab_size), 1, 16);
    void patchConfig((prev) => ({ ...prev, editor: { ...prev.editor, tab_size: value } }));
  }

  return (
    <div data-section="editor">
      <div class="settings-section-label">Editor</div>
      <div class="settings-row">
        <label class="settings-row-label" for="setting-font-size">Font size</label>
        <input
          id="setting-font-size"
          type="number"
          class="settings-input settings-input-number"
          data-setting="font_size"
          value={cfg().font_size}
          min={8}
          max={72}
          onChange={(e) => onFontSizeChange(e.currentTarget.value)}
        />
      </div>
      <div class="settings-row">
        <label class="settings-row-label" for="setting-tab-size">Tab size</label>
        <input
          id="setting-tab-size"
          type="number"
          class="settings-input settings-input-number"
          data-setting="tab_size"
          value={cfg().tab_size}
          min={1}
          max={16}
          onChange={(e) => onTabSizeChange(e.currentTarget.value)}
        />
      </div>
      <div class="settings-row">
        <span class="settings-row-label">Word wrap</span>
        <button
          type="button"
          class="settings-toggle"
          classList={{ "settings-toggle-on": cfg().word_wrap }}
          data-setting="word_wrap"
          role="switch"
          aria-checked={cfg().word_wrap}
          onClick={onWordWrapToggle}
        >
          <span class="settings-toggle-thumb" />
        </button>
      </div>
    </div>
  );
}

function FilesSection() {
  const cfg = () => configStore.config().editor;
  const [isInstallingCli, setIsInstallingCli] = createSignal(false);

  function onAutosaveChange(raw: string) {
    const value = clamp(parseIntSafe(raw, cfg().autosave_debounce_ms), 0, 10000);
    void patchConfig((prev) => ({ ...prev, editor: { ...prev.editor, autosave_debounce_ms: value } }));
  }

  async function onInstallCli() {
    if (isInstallingCli()) return;
    setIsInstallingCli(true);
    try {
      const result = await installCli();
      showToast(`writ installed at ${result.symlink_path}`, "success");
    } catch (err) {
      const detail = typeof err === "string" ? err : String(err);
      showToast(`Install failed: ${detail}`, "error");
    } finally {
      setIsInstallingCli(false);
    }
  }

  return (
    <div data-section="files">
      <div class="settings-section-label">Files</div>
      <div class="settings-row">
        <label class="settings-row-label" for="setting-autosave">Autosave delay (ms)</label>
        <input
          id="setting-autosave"
          type="number"
          class="settings-input settings-input-number"
          data-setting="autosave_debounce_ms"
          value={cfg().autosave_debounce_ms}
          min={0}
          max={10000}
          onChange={(e) => onAutosaveChange(e.currentTarget.value)}
        />
      </div>
      <div class="settings-row">
        <span class="settings-row-label">Command-line tool</span>
        <button
          type="button"
          class="settings-action-btn"
          data-action="install-cli"
          disabled={isInstallingCli()}
          onClick={() => void onInstallCli()}
        >
          {isInstallingCli() ? "Installing…" : "Install `writ` command"}
        </button>
      </div>
    </div>
  );
}

function PreviewSection() {
  const cfg = () => configStore.config().preview;

  function onLiveThresholdChange(raw: string) {
    const value = clamp(parseFloatSafe(raw, cfg().live_render_threshold_mb), 0.1, 100);
    void patchConfig((prev) => ({ ...prev, preview: { ...prev.preview, live_render_threshold_mb: value } }));
  }

  function onRefuseThresholdChange(raw: string) {
    const value = clamp(parseFloatSafe(raw, cfg().render_refuse_threshold_mb), 1, 500);
    void patchConfig((prev) => ({ ...prev, preview: { ...prev.preview, render_refuse_threshold_mb: value } }));
  }

  function onRunScriptsToggle() {
    void patchConfig((prev) => ({ ...prev, preview: { ...prev.preview, run_scripts: !prev.preview.run_scripts } }));
  }

  function onDefaultLayoutHtmlChange(raw: string) {
    const layout = raw as DefaultLayout;
    void patchConfig((prev) => ({ ...prev, preview: { ...prev.preview, default_layout_html: layout } }));
  }

  function onDefaultLayoutMarkdownChange(raw: string) {
    const layout = raw as DefaultLayout;
    void patchConfig((prev) => ({ ...prev, preview: { ...prev.preview, default_layout_markdown: layout } }));
  }

  return (
    <div data-section="preview">
      <div class="settings-section-label">Preview</div>
      <div class="settings-row">
        <label class="settings-row-label" for="setting-live-threshold">Live render threshold (MB)</label>
        <input
          id="setting-live-threshold"
          type="number"
          class="settings-input settings-input-number"
          data-setting="live_render_threshold_mb"
          value={cfg().live_render_threshold_mb}
          min={0.1}
          max={100}
          step={0.5}
          onChange={(e) => onLiveThresholdChange(e.currentTarget.value)}
        />
      </div>
      <div class="settings-row">
        <label class="settings-row-label" for="setting-refuse-threshold">Refuse render threshold (MB)</label>
        <input
          id="setting-refuse-threshold"
          type="number"
          class="settings-input settings-input-number"
          data-setting="render_refuse_threshold_mb"
          value={cfg().render_refuse_threshold_mb}
          min={1}
          max={500}
          step={1}
          onChange={(e) => onRefuseThresholdChange(e.currentTarget.value)}
        />
      </div>
      <div class="settings-row">
        <span class="settings-row-label">Run scripts by default</span>
        <button
          type="button"
          class="settings-toggle"
          classList={{ "settings-toggle-on": cfg().run_scripts }}
          data-setting="run_scripts"
          role="switch"
          aria-checked={cfg().run_scripts}
          onClick={onRunScriptsToggle}
        >
          <span class="settings-toggle-thumb" />
        </button>
      </div>
      <div class="settings-row">
        <label class="settings-row-label" for="setting-layout-html">HTML default layout</label>
        <select
          id="setting-layout-html"
          class="settings-select"
          data-setting="default_layout_html"
          value={cfg().default_layout_html}
          onChange={(e) => onDefaultLayoutHtmlChange(e.currentTarget.value)}
        >
          <option value="source">Source only</option>
          <option value="split">Split</option>
          <option value="preview">Preview only</option>
        </select>
      </div>
      <div class="settings-row">
        <label class="settings-row-label" for="setting-layout-md">Markdown default layout</label>
        <select
          id="setting-layout-md"
          class="settings-select"
          data-setting="default_layout_markdown"
          value={cfg().default_layout_markdown}
          onChange={(e) => onDefaultLayoutMarkdownChange(e.currentTarget.value)}
        >
          <option value="source">Source only</option>
          <option value="split">Split</option>
          <option value="preview">Preview only</option>
        </select>
      </div>
    </div>
  );
}

function AppearanceSection() {
  const currentPreset = () => configStore.config().theme.preset;

  function onPresetChange(id: string) {
    themeStore.setPreset(id);
    void patchConfig((prev) => ({ ...prev, theme: { ...prev.theme, preset: id } }));
  }

  return (
    <div data-section="appearance">
      <div class="settings-section-label">Appearance</div>
      <div class="settings-row">
        <label class="settings-row-label" for="setting-theme-preset">Theme</label>
        <select
          id="setting-theme-preset"
          class="settings-select"
          data-setting="theme_preset"
          value={currentPreset()}
          onChange={(e) => onPresetChange(e.currentTarget.value)}
        >
          {PRESETS.map((p) => (
            <option value={p.id}>{p.name}</option>
          ))}
        </select>
      </div>
      <div class="settings-row">
        <span class="settings-row-label">Custom colors</span>
        <button
          type="button"
          class="settings-action-btn"
          data-action="open-theme-editor"
          onClick={() => {
            closeSettings();
            openThemeEditor();
          }}
        >
          Edit theme…
        </button>
      </div>
    </div>
  );
}

function ShortcutsSection() {
  return (
    <div data-section="shortcuts">
      <div class="settings-section-label">Shortcuts</div>
      <div class="settings-row">
        <span class="settings-row-label">Keyboard shortcuts</span>
        <button
          type="button"
          class="settings-action-btn"
          data-action="open-shortcut-editor"
          onClick={() => {
            closeSettings();
            openShortcutEditor();
          }}
        >
          Edit shortcuts…
        </button>
      </div>
    </div>
  );
}

export default function SettingsModal() {
  const win = useWindow();
  let modalRef: HTMLDivElement | undefined;
  const titleId = "settings-modal-title";

  createEffect(() => {
    if (!isOpen() || !modalRef) return;
    const teardown = installFocusTrap(modalRef, {
      onEscape: () => closeSettings(),
      fallbackRestore: () => {
        win.editor.focusEditor();
        return null;
      },
    });
    onCleanup(teardown);
  });

  return (
    <Show when={isOpen()}>
      <div class="settings-overlay" onClick={() => closeSettings()}>
        <div
          ref={modalRef}
          class="settings-modal"
          onClick={(e) => e.stopPropagation()}
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
        >
          <div class="settings-header">
            <span id={titleId} class="settings-title">Settings</span>
            <button
              type="button"
              class="settings-close"
              onClick={closeSettings}
              aria-label="Close settings"
              title="Close"
            >
              <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
                <path
                  d="M2 2L10 10M10 2L2 10"
                  stroke="currentColor"
                  stroke-width="1.5"
                  stroke-linecap="round"
                />
              </svg>
            </button>
          </div>

          <div class="settings-body">
            <nav class="settings-nav" aria-label="Settings sections">
              {NAV_ITEMS.map((item) => (
                <button
                  type="button"
                  class="settings-nav-item"
                  classList={{ "settings-nav-item-active": activeSection() === item.id }}
                  onClick={() => setActiveSection(item.id)}
                  aria-current={activeSection() === item.id ? "page" : undefined}
                >
                  {item.label}
                </button>
              ))}
            </nav>

            <div class="settings-content">
              <Switch>
                <Match when={activeSection() === "editor"}>
                  <EditorSection />
                </Match>
                <Match when={activeSection() === "files"}>
                  <FilesSection />
                </Match>
                <Match when={activeSection() === "preview"}>
                  <PreviewSection />
                </Match>
                <Match when={activeSection() === "appearance"}>
                  <AppearanceSection />
                </Match>
                <Match when={activeSection() === "shortcuts"}>
                  <ShortcutsSection />
                </Match>
              </Switch>
            </div>
          </div>
        </div>
      </div>
    </Show>
  );
}
