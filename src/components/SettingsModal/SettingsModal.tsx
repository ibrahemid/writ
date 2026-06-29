import { createSignal, createEffect, onCleanup, onMount, For, Show, Switch, Match } from "solid-js";
import { configStore, EDITOR_FONT_MIN, EDITOR_FONT_MAX } from "../../stores/global/config";
import { inboxStore } from "../../stores/global/inbox";
import { themeStore } from "../../stores/global/theme";
import { updateStore } from "../../stores/global/update";
import { PRESETS } from "../../styles/themes";
import { openThemeEditor } from "../ThemeEditor/ThemeEditor";
import { openShortcutEditor } from "../ShortcutEditor/ShortcutEditor";
import { installFocusTrap } from "../../lib/focus-trap";
import { useWindow } from "../WindowProvider/WindowProvider";
import { showToast } from "../Notifications/Toast";
import { installCli } from "../../stores/global/cli";
import type { DefaultLayout } from "../../types/config";
import {
  fetchDefaultAppTypes,
  fetchDefaultAppStatus,
  claimDefaultApp,
} from "../../stores/global/default-app";
import type { ClaimableType, DefaultAppStatus } from "../../stores/global/default-app";
import "./SettingsModal.css";

type Section = "editor" | "files" | "preview" | "appearance" | "shortcuts" | "updates";

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
  { id: "updates", label: "Updates" },
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
    const value = clamp(parseIntSafe(raw, cfg().font_size), EDITOR_FONT_MIN, EDITOR_FONT_MAX);
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
          min={EDITOR_FONT_MIN}
          max={EDITOR_FONT_MAX}
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

const REFRESH_DELAY_MS = 800;

interface DefaultAppRowProps {
  type: ClaimableType;
}

function DefaultAppRow(props: DefaultAppRowProps) {
  const [status, setStatus] = createSignal<DefaultAppStatus | null>(null);
  const [setting, setSetting] = createSignal(false);
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;

  const extList = () => props.type.exts.map((e) => `.${e}`).join(", ");

  async function loadStatus() {
    try {
      const s = await fetchDefaultAppStatus(props.type.id);
      setStatus(s);
    } catch {
      // Non-macOS or sandboxed; treat as unsupported
      setStatus({ status: "unsupported" });
    }
  }

  onMount(() => {
    void loadStatus();
  });

  onCleanup(() => {
    if (refreshTimer !== undefined) clearTimeout(refreshTimer);
  });

  async function onMakeDefault() {
    setSetting(true);
    try {
      await claimDefaultApp(props.type.id);
    } catch (err) {
      showToast(`Failed to set default for ${props.type.label}`, "error");
      // A group spans several UTIs; a mid-loop failure may have claimed some.
      // Re-query so the row reflects the real partial state, not a stale one.
      void loadStatus().finally(() => setSetting(false));
      return;
    }
    // LS registration is async at the OS level — re-query after a short delay
    // and reflect the actual registered handler rather than assuming success.
    refreshTimer = setTimeout(() => {
      void loadStatus().finally(() => setSetting(false));
    }, REFRESH_DELAY_MS);
  }

  const currentStatus = () => status();

  return (
    <Show when={currentStatus() !== null && currentStatus()!.status !== "unsupported"}>
      <div class="settings-row">
        <span class="settings-row-label">
          {props.type.label}
          <span class="settings-row-caution">{extList()}</span>
        </span>
        <div class="settings-default-app-ctrl">
          <span
            class="settings-default-app-status"
            classList={{
              "settings-default-app-status-active": currentStatus()?.status === "is_default",
            }}
            aria-live="polite"
          >
            {currentStatus()?.status === "is_default"
              ? "Writ is the default"
              : currentStatus()?.status === "other_app"
                ? (currentStatus() as Extract<DefaultAppStatus, { status: "other_app" }>).name
                  ? `${(currentStatus() as Extract<DefaultAppStatus, { status: "other_app" }>).name} is the default`
                  : "Another app is the default"
                : "No default set"}
          </span>
          <Show when={currentStatus()?.status !== "is_default"}>
            <button
              type="button"
              class="settings-action-btn"
              data-action={`make-default-${props.type.id}`}
              disabled={setting()}
              aria-busy={setting()}
              aria-label={`Make Writ the default app for ${props.type.label}`}
              onClick={() => void onMakeDefault()}
            >
              {setting() ? "Setting…" : "Make default"}
            </button>
          </Show>
        </div>
      </div>
    </Show>
  );
}

function FilesSection() {
  const cfg = () => configStore.config().editor;
  const [isInstallingCli, setIsInstallingCli] = createSignal(false);
  const inboxPath = () => inboxStore.path();
  const inboxFocus = () => configStore.config().inbox.focus;
  const [defaultAppTypes, setDefaultAppTypes] = createSignal<ClaimableType[]>([]);

  onMount(() => {
    void fetchDefaultAppTypes()
      .then(setDefaultAppTypes)
      .catch(() => setDefaultAppTypes([]));
  });

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

  function onInboxFocusToggle() {
    void patchConfig((prev) => ({ ...prev, inbox: { ...prev.inbox, focus: !prev.inbox.focus } }));
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
      <For each={defaultAppTypes()}>{(t) => <DefaultAppRow type={t} />}</For>
      <div class="settings-row">
        <span class="settings-row-label">Watched inbox folder</span>
        <Show
          when={inboxPath()}
          fallback={
            <button
              type="button"
              class="settings-action-btn"
              data-action="inbox-watch"
              onClick={() => void inboxStore.watchFolder()}
            >
              Watch folder…
            </button>
          }
        >
          {(path) => (
            <span class="settings-inbox-controls">
              <span class="settings-inbox-path" title={path()}>{path()}</span>
              <button
                type="button"
                class="settings-action-btn"
                data-action="inbox-change"
                onClick={() => void inboxStore.watchFolder()}
              >
                Change…
              </button>
              <button
                type="button"
                class="settings-action-btn"
                data-action="inbox-clear"
                onClick={() => void inboxStore.stopWatching()}
              >
                Clear
              </button>
            </span>
          )}
        </Show>
      </div>
      <div class="settings-row">
        <span class="settings-row-label">Focus window on inbox open</span>
        <button
          type="button"
          class="settings-toggle"
          classList={{ "settings-toggle-on": inboxFocus() }}
          data-setting="inbox_focus"
          role="switch"
          aria-checked={inboxFocus()}
          onClick={onInboxFocusToggle}
        >
          <span class="settings-toggle-thumb" />
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

function UpdatesSection() {
  const autoCheck = () => configStore.config().updater.auto_check;

  function onAutoCheckToggle() {
    void patchConfig((prev) => ({
      ...prev,
      updater: { ...prev.updater, auto_check: !prev.updater.auto_check },
    }));
  }

  return (
    <div data-section="updates">
      <div class="settings-section-label">Updates</div>
      <div class="settings-row">
        <span class="settings-row-label">Check for updates automatically</span>
        <button
          type="button"
          class="settings-toggle"
          classList={{ "settings-toggle-on": autoCheck() }}
          data-setting="updater_auto_check"
          role="switch"
          aria-checked={autoCheck()}
          onClick={onAutoCheckToggle}
        >
          <span class="settings-toggle-thumb" />
        </button>
      </div>
      <div class="settings-row">
        <span class="settings-row-label">Check for updates now</span>
        <button
          type="button"
          class="settings-action-btn"
          data-action="check-updates-now"
          onClick={() => void updateStore.checkForUpdate()}
        >
          Check now
        </button>
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
                <Match when={activeSection() === "updates"}>
                  <UpdatesSection />
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
