import {
  createSignal,
  createEffect,
  createContext,
  useContext,
  onCleanup,
  onMount,
  For,
  Show,
  Switch,
  Match,
  type JSX,
} from "solid-js";
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
import { fetchCliStatus, installCli } from "../../stores/global/cli";
import { copyStoragePath, fetchStorageInfo, revealStoragePath } from "../../stores/global/storage";
import type { StorageInfo } from "../../stores/global/storage";
import type { DefaultLayout } from "../../types/config";
import {
  fetchDefaultAppTypes,
  fetchDefaultAppStatus,
  claimDefaultApp,
} from "../../stores/global/default-app";
import type { ClaimableType, DefaultAppStatus } from "../../stores/global/default-app";
import { markDefaultAppTypeSupported } from "../../stores/global/default-app-support";
import {
  SECTION_LABELS,
  SECTION_ORDER,
  defaultAppSettingId,
  matchedSettingIds,
  rankSettings,
  type SettingsSection,
} from "../../settings";
import { isSettingAvailable } from "../../settings/availability";
import "./SettingsModal.css";

// Singleton state — Writ is single-window
const [isOpen, setIsOpen] = createSignal(false);
const [activeSection, setActiveSection] = createSignal<SettingsSection>("editor");
const [query, setQuery] = createSignal("");
const [highlightId, setHighlightId] = createSignal<string | null>(null);

export function openSettings(section?: SettingsSection, settingId?: string) {
  setQuery("");
  setActiveSection(section ?? "editor");
  setHighlightId(settingId ?? null);
  setIsOpen(true);
}

export function closeSettings() {
  setHighlightId(null);
  setIsOpen(false);
}

export function toggleSettings() {
  if (isOpen()) closeSettings();
  else openSettings();
}

const isSearching = () => query().trim().length > 0;

interface SearchContextValue {
  rowVisible: (id: string) => boolean;
  sectionVisible: (section: SettingsSection) => boolean;
  highlighted: (id: string) => boolean;
}

const SearchContext = createContext<SearchContextValue>();

function useSearch(): SearchContextValue {
  const ctx = useContext(SearchContext);
  if (!ctx) throw new Error("SettingsRow used outside SettingsModal");
  return ctx;
}

const NAV_ITEMS: { id: SettingsSection; label: string }[] = SECTION_ORDER.map((id) => ({
  id,
  label: SECTION_LABELS[id],
}));

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

interface SettingsRowProps {
  id: string;
  label: string;
  labelFor?: string;
  caution?: string;
  children: JSX.Element;
}

function SettingsRow(props: SettingsRowProps) {
  const search = useSearch();
  return (
    <Show when={search.rowVisible(props.id)}>
      <div
        class="settings-row"
        classList={{ "settings-row-highlight": search.highlighted(props.id) }}
        data-setting-id={props.id}
      >
        <Show
          when={props.labelFor}
          fallback={
            <span class="settings-row-label">
              {props.label}
              <Show when={props.caution}>
                <span class="settings-row-caution">{props.caution}</span>
              </Show>
            </span>
          }
        >
          <label class="settings-row-label" for={props.labelFor}>
            {props.label}
          </label>
        </Show>
        {props.children}
      </div>
    </Show>
  );
}

function SectionLabel(props: { section: SettingsSection }) {
  const search = useSearch();
  return (
    <Show when={search.sectionVisible(props.section)}>
      <div class="settings-section-label">{SECTION_LABELS[props.section]}</div>
    </Show>
  );
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

  function onMarkdownTypographyToggle() {
    void patchConfig((prev) => ({
      ...prev,
      editor: { ...prev.editor, markdown_typography: !prev.editor.markdown_typography },
    }));
  }

  function onMarkdownEditingToggle() {
    void patchConfig((prev) => ({
      ...prev,
      editor: { ...prev.editor, markdown_editing: !prev.editor.markdown_editing },
    }));
  }

  const spelling = () => configStore.config().spelling;

  function onSpellingToggle() {
    void patchConfig((prev) => ({
      ...prev,
      spelling: { ...prev.spelling, enabled: !prev.spelling.enabled },
    }));
  }

  function onSpellingDialectChange(raw: string) {
    void patchConfig((prev) => ({ ...prev, spelling: { ...prev.spelling, dialect: raw } }));
  }

  return (
    <div data-section="editor">
      <SectionLabel section="editor" />
      <SettingsRow id="editor.font_size" label="Font size" labelFor="setting-font-size">
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
      </SettingsRow>
      <SettingsRow id="editor.tab_size" label="Tab size" labelFor="setting-tab-size">
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
      </SettingsRow>
      <SettingsRow id="editor.word_wrap" label="Word wrap">
        <button
          type="button"
          class="settings-toggle"
          classList={{ "settings-toggle-on": cfg().word_wrap }}
          data-setting="word_wrap"
          role="switch"
          aria-checked={cfg().word_wrap}
          aria-label="Word wrap"
          onClick={onWordWrapToggle}
        >
          <span class="settings-toggle-thumb" />
        </button>
      </SettingsRow>
      <SettingsRow id="editor.markdown_typography" label="Markdown typography">
        <button
          type="button"
          class="settings-toggle"
          classList={{ "settings-toggle-on": cfg().markdown_typography }}
          data-setting="markdown_typography"
          role="switch"
          aria-checked={cfg().markdown_typography}
          aria-label="Markdown typography"
          onClick={onMarkdownTypographyToggle}
        >
          <span class="settings-toggle-thumb" />
        </button>
      </SettingsRow>
      <SettingsRow id="editor.markdown_editing" label="Markdown editing helpers">
        <button
          type="button"
          class="settings-toggle"
          classList={{ "settings-toggle-on": cfg().markdown_editing }}
          data-setting="markdown_editing"
          role="switch"
          aria-checked={cfg().markdown_editing}
          aria-label="Markdown editing helpers"
          onClick={onMarkdownEditingToggle}
        >
          <span class="settings-toggle-thumb" />
        </button>
      </SettingsRow>
      <SettingsRow id="editor.spelling" label="Spell check">
        <button
          type="button"
          class="settings-toggle"
          classList={{ "settings-toggle-on": spelling().enabled }}
          data-setting="spelling_enabled"
          role="switch"
          aria-checked={spelling().enabled}
          aria-label="Spell check"
          onClick={onSpellingToggle}
        >
          <span class="settings-toggle-thumb" />
        </button>
      </SettingsRow>
      <SettingsRow id="editor.spelling_dialect" label="Spelling dialect" labelFor="setting-spelling-dialect">
        <select
          id="setting-spelling-dialect"
          class="settings-select"
          data-setting="spelling_dialect"
          value={spelling().dialect}
          onChange={(e) => onSpellingDialectChange(e.currentTarget.value)}
        >
          <option value="american">American</option>
          <option value="british">British</option>
          <option value="canadian">Canadian</option>
          <option value="australian">Australian</option>
        </select>
      </SettingsRow>
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

  const settingId = () => defaultAppSettingId(props.type.id);
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

  // Keep the shared support registry fresh as this row resolves (covers a
  // post-startup change, e.g. after "Make default"), so search headers and the
  // command palette never offer a row that will not render on this platform.
  createEffect(() => {
    const s = status();
    if (s === null) return;
    markDefaultAppTypeSupported(props.type.id, s.status !== "unsupported");
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
      <SettingsRow id={settingId()} label={props.type.label} caution={extList()}>
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
      </SettingsRow>
    </Show>
  );
}

function FilesSection() {
  const cfg = () => configStore.config().editor;
  const [isInstallingCli, setIsInstallingCli] = createSignal(false);
  const [cliInstalled, setCliInstalled] = createSignal(false);
  const inboxPath = () => inboxStore.path();
  const inboxFocus = () => configStore.config().inbox.focus;
  const [defaultAppTypes, setDefaultAppTypes] = createSignal<ClaimableType[]>([]);

  function refreshCliStatus() {
    void fetchCliStatus()
      .then((s) => setCliInstalled(s.installed))
      .catch(() => setCliInstalled(false));
  }

  onMount(() => {
    refreshCliStatus();
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
      setCliInstalled(true);
      showToast(`writ installed at ${result.symlink_path}`, "success");
    } catch (err) {
      const detail = typeof err === "string" ? err : String(err);
      showToast(detail, "error");
      refreshCliStatus();
    } finally {
      setIsInstallingCli(false);
    }
  }

  function onInboxFocusToggle() {
    void patchConfig((prev) => ({ ...prev, inbox: { ...prev.inbox, focus: !prev.inbox.focus } }));
  }

  return (
    <div data-section="files">
      <SectionLabel section="files" />
      <SettingsRow id="files.autosave" label="Autosave delay (ms)" labelFor="setting-autosave">
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
      </SettingsRow>
      <SettingsRow id="files.cli" label="Command-line tool">
        <Show
          when={!cliInstalled()}
          fallback={
            <span class="settings-default-app-status settings-default-app-status-active">
              writ command installed
            </span>
          }
        >
          <button
            type="button"
            class="settings-action-btn"
            data-action="install-cli"
            disabled={isInstallingCli()}
            onClick={() => void onInstallCli()}
          >
            {isInstallingCli() ? "Installing…" : "Install `writ` command"}
          </button>
        </Show>
      </SettingsRow>
      <For each={defaultAppTypes()}>{(t) => <DefaultAppRow type={t} />}</For>
      <SettingsRow id="files.inbox_folder" label="Watched inbox folder">
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
      </SettingsRow>
      <SettingsRow id="files.inbox_focus" label="Focus window on inbox open">
        <button
          type="button"
          class="settings-toggle"
          classList={{ "settings-toggle-on": inboxFocus() }}
          data-setting="inbox_focus"
          role="switch"
          aria-checked={inboxFocus()}
          aria-label="Focus window on inbox open"
          onClick={onInboxFocusToggle}
        >
          <span class="settings-toggle-thumb" />
        </button>
      </SettingsRow>
    </div>
  );
}

function StorageSection() {
  const [info, setInfo] = createSignal<StorageInfo | null>(null);

  onMount(() => {
    void fetchStorageInfo()
      .then(setInfo)
      .catch(() => setInfo(null));
  });

  async function onReveal() {
    try {
      await revealStoragePath();
    } catch {
      showToast("Could not open the file manager", "error");
    }
  }

  async function onCopy() {
    const path = info()?.db_path;
    if (!path) return;
    try {
      await copyStoragePath(path);
      showToast("Path copied", "success");
    } catch {
      showToast("Could not copy the path", "error");
    }
  }

  return (
    <div data-section="storage">
      <SectionLabel section="storage" />
      <SettingsRow id="storage.location" label="Storage location">
        <span class="settings-inbox-controls">
          <span class="settings-inbox-path" data-storage-path title={info()?.db_path ?? ""}>
            {info()?.db_path ?? "…"}
          </span>
          <button
            type="button"
            class="settings-action-btn"
            data-action="storage-reveal"
            onClick={() => void onReveal()}
          >
            Reveal
          </button>
          <button
            type="button"
            class="settings-action-btn"
            data-action="storage-copy"
            onClick={() => void onCopy()}
          >
            Copy
          </button>
        </span>
      </SettingsRow>
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
      <SectionLabel section="preview" />
      <SettingsRow id="preview.live_threshold" label="Live render threshold (MB)" labelFor="setting-live-threshold">
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
      </SettingsRow>
      <SettingsRow id="preview.refuse_threshold" label="Refuse render threshold (MB)" labelFor="setting-refuse-threshold">
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
      </SettingsRow>
      <SettingsRow id="preview.run_scripts" label="Run scripts by default">
        <button
          type="button"
          class="settings-toggle"
          classList={{ "settings-toggle-on": cfg().run_scripts }}
          data-setting="run_scripts"
          role="switch"
          aria-checked={cfg().run_scripts}
          aria-label="Run scripts by default"
          onClick={onRunScriptsToggle}
        >
          <span class="settings-toggle-thumb" />
        </button>
      </SettingsRow>
      <SettingsRow id="preview.layout_html" label="HTML default layout" labelFor="setting-layout-html">
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
      </SettingsRow>
      <SettingsRow id="preview.layout_md" label="Markdown default layout" labelFor="setting-layout-md">
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
      </SettingsRow>
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
      <SectionLabel section="updates" />
      <SettingsRow id="updates.auto_check" label="Check for updates automatically">
        <button
          type="button"
          class="settings-toggle"
          classList={{ "settings-toggle-on": autoCheck() }}
          data-setting="updater_auto_check"
          role="switch"
          aria-checked={autoCheck()}
          aria-label="Check for updates automatically"
          onClick={onAutoCheckToggle}
        >
          <span class="settings-toggle-thumb" />
        </button>
      </SettingsRow>
      <SettingsRow id="updates.check_now" label="Check for updates now">
        <button
          type="button"
          class="settings-action-btn"
          data-action="check-updates-now"
          onClick={() => void updateStore.checkForUpdate()}
        >
          Check now
        </button>
      </SettingsRow>
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
      <SectionLabel section="appearance" />
      <SettingsRow id="appearance.theme" label="Theme" labelFor="setting-theme-preset">
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
      </SettingsRow>
      <SettingsRow id="appearance.custom_colors" label="Custom colors">
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
      </SettingsRow>
    </div>
  );
}

function ShortcutsSection() {
  return (
    <div data-section="shortcuts">
      <SectionLabel section="shortcuts" />
      <SettingsRow id="shortcuts.edit" label="Keyboard shortcuts">
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
      </SettingsRow>
    </div>
  );
}

function AllSections() {
  return (
    <>
      <EditorSection />
      <FilesSection />
      <StorageSection />
      <PreviewSection />
      <AppearanceSection />
      <UpdatesSection />
      <ShortcutsSection />
    </>
  );
}

export default function SettingsModal() {
  const win = useWindow();
  let modalRef: HTMLDivElement | undefined;
  let contentRef: HTMLDivElement | undefined;
  let searchRef: HTMLInputElement | undefined;
  const titleId = "settings-modal-title";

  const matched = () => matchedSettingIds(query());

  // Sections that have at least one currently-renderable matching row.
  const matchedSections = () => {
    const result = new Set<SettingsSection>();
    if (!isSearching()) return result;
    for (const entry of rankSettings(query())) {
      if (isSettingAvailable(entry.id)) result.add(entry.section);
    }
    return result;
  };

  const searchContext: SearchContextValue = {
    rowVisible: (id) => !isSearching() || (matched().has(id) && isSettingAvailable(id)),
    sectionVisible: (section) => !isSearching() || matchedSections().has(section),
    highlighted: (id) => highlightId() === id,
  };

  const noMatches = () =>
    isSearching() && rankSettings(query()).every((entry) => !isSettingAvailable(entry.id));

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

  createEffect(() => {
    if (isOpen() && searchRef) {
      requestAnimationFrame(() => searchRef!.focus());
    }
  });

  createEffect(() => {
    const id = highlightId();
    if (!isOpen() || !id || !contentRef) return;
    const target = contentRef;
    requestAnimationFrame(() => {
      const el = target.querySelector<HTMLElement>(`[data-setting-id="${id}"]`);
      if (el && typeof el.scrollIntoView === "function") {
        el.scrollIntoView({ block: "center" });
      }
    });
    const timer = setTimeout(() => setHighlightId(null), 1200);
    onCleanup(() => clearTimeout(timer));
  });

  return (
    <Show when={isOpen()}>
      <SearchContext.Provider value={searchContext}>
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

            <div class="settings-search-bar">
              <svg class="settings-search-icon" width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
                <circle cx="6" cy="6" r="4.25" fill="none" stroke="currentColor" stroke-width="1.4" />
                <path d="M9.2 9.2L12 12" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
              </svg>
              <input
                ref={searchRef}
                type="text"
                class="settings-search-input"
                placeholder="Search settings"
                value={query()}
                onInput={(e) => setQuery(e.currentTarget.value)}
                aria-label="Search settings"
              />
              <Show when={isSearching()}>
                <button
                  type="button"
                  class="settings-search-clear"
                  onClick={() => {
                    setQuery("");
                    searchRef?.focus();
                  }}
                  aria-label="Clear search"
                  title="Clear search"
                >
                  <svg width="10" height="10" viewBox="0 0 12 12" aria-hidden="true">
                    <path d="M2 2L10 10M10 2L2 10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
                  </svg>
                </button>
              </Show>
            </div>

            <div class="settings-body">
              <Show when={!isSearching()}>
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
              </Show>

              <div
                ref={contentRef}
                class="settings-content"
                classList={{ "settings-content-search": isSearching() }}
              >
                <Show
                  when={isSearching()}
                  fallback={
                    <Switch>
                      <Match when={activeSection() === "editor"}><EditorSection /></Match>
                      <Match when={activeSection() === "files"}><FilesSection /></Match>
                      <Match when={activeSection() === "storage"}><StorageSection /></Match>
                      <Match when={activeSection() === "preview"}><PreviewSection /></Match>
                      <Match when={activeSection() === "appearance"}><AppearanceSection /></Match>
                      <Match when={activeSection() === "updates"}><UpdatesSection /></Match>
                      <Match when={activeSection() === "shortcuts"}><ShortcutsSection /></Match>
                    </Switch>
                  }
                >
                  <Show
                    when={!noMatches()}
                    fallback={
                      <div class="settings-empty">
                        <div class="settings-empty-title">No settings match "{query()}"</div>
                        <div class="settings-empty-hint">Try a different word, or clear the search.</div>
                      </div>
                    }
                  >
                    <AllSections />
                  </Show>
                </Show>
              </div>
            </div>
          </div>
        </div>
      </SearchContext.Provider>
    </Show>
  );
}
