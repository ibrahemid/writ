import {
  createSignal,
  createEffect,
  createContext,
  createUniqueId,
  useContext,
  onCleanup,
  onMount,
  For,
  Show,
  Switch,
  Match,
  type JSX,
} from "solid-js";
import {
  configStore,
  EDITOR_FONT_MIN,
  EDITOR_FONT_MAX,
  INTERFACE_TEXT_MIN,
  INTERFACE_TEXT_MAX,
  clampInterfaceTextSize,
} from "../../stores/global/config";
import { inboxStore } from "../../stores/global/inbox";
import { themeStore } from "../../stores/global/theme";
import { updateStore } from "../../stores/global/update";
import { PRESETS } from "../../styles/themes";
import { openThemeEditor } from "../ThemeEditor/ThemeEditor";
import { openShortcutEditor } from "../ShortcutEditor/ShortcutEditor";
import { installFocusTrap } from "../../lib/focus-trap";
import { joinAnd } from "../../lib/join-and";
import { logFailure } from "../../lib/log";
import { FILE_MANAGER_NAME, IS_MAC, SHOW_IN_FILE_MANAGER } from "../../lib/platform";
import { useWindow } from "../WindowProvider/WindowProvider";
import { showToast } from "../Notifications/Toast";
import { fetchCliStatus, installCli } from "../../stores/global/cli";
import {
  aiRewriteStore,
  type AiKeyState,
  type AiEndpointState,
} from "../../stores/global/ai-rewrite";
import {
  aiConnectionStore,
  connectionDisplay,
  modelListDisplay,
  CHOOSE_A_MODEL,
  type LocalProbe,
  type ModelListError,
} from "../../stores/global/ai-connection";
import { aiProvidersStore } from "../../stores/global/ai-providers";
import { modelOptions, resolveAutoModel } from "../../stores/global/ai-models";
import { linkStore } from "../../stores/global/link";
import { notesStore } from "../../stores/global/notes";
import type { NotesFallbackReason } from "../../stores/global/notes";
import { NotesSyncNote } from "./NotesSyncNote";
import { copyStoragePath, fetchStorageInfo, revealStoragePath } from "../../stores/global/storage";
import { activityStore } from "../../stores/global/activity";
import { openActivity } from "../Activity/ActivityPanel";
import type { StorageInfo } from "../../stores/global/storage";
import type {
  AccentId,
  AppearanceConfig,
  DefaultLayout,
  FileExtension,
  Polarity,
  ProseFaceId,
  SidebarSectionId,
} from "../../types/config";
import {
  fetchDefaultAppTypes,
  fetchDefaultAppStatus,
  claimDefaultApp,
} from "../../stores/global/default-app";
import type { ClaimableType, DefaultAppStatus } from "../../stores/global/default-app";
import {
  hasSupportedDefaultAppTypes,
  markDefaultAppTypeSupported,
} from "../../stores/global/default-app-support";
import {
  DEFAULT_APP_SETTING_ID,
  SECTION_LABELS,
  SECTION_ORDER,
  matchedSettingIds,
  rankSettings,
  type SettingsSection,
} from "../../settings";
import { isSectionAvailable, isSettingAvailable } from "../../settings/availability";
import Button from "../Button/Button";
import Icon from "../Icon/Icon";
import Tooltip from "../Tooltip/Tooltip";
import { ACCENTS, TYPE } from "../../styles/generated/tokens";
import { resolvePlatform } from "../../lib/platform";
import "./SettingsModal.css";

// Singleton state — Writ is single-window

// Settings opens on the section the nav rail leads with, which is the one
// answering "where are my notes" (ADR-028 section 2). Every caller that means
// a particular section names it, so this only decides where a bare Cmd+, or
// menu item lands.
const DEFAULT_SECTION: SettingsSection = SECTION_ORDER[0];

const [isOpen, setIsOpen] = createSignal(false);
const [activeSection, setActiveSection] = createSignal<SettingsSection>(DEFAULT_SECTION);
const [query, setQuery] = createSignal("");
const [highlightId, setHighlightId] = createSignal<string | null>(null);

export function openSettings(section?: SettingsSection, settingId?: string) {
  setQuery("");
  // A section the platform cannot fill has no nav item, so landing on one would
  // leave no way back out of it.
  const target = section && isSectionAvailable(section) ? section : DEFAULT_SECTION;
  setActiveSection(target);
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

const NAV_ITEMS: { id: SettingsSection; label: string }[] = SECTION_ORDER.filter(
  isSectionAvailable,
).map((id) => ({ id, label: SECTION_LABELS[id] }));

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
    showToast("Could not save your settings", "error");
  }
}

interface ToggleSwitchProps {
  setting: string;
  label: string;
  checked: boolean;
  disabled?: boolean;
  onChange: () => void;
}

/** 32 x 18 track, 14px knob. A button, so Space and Enter activate it natively. */
function ToggleSwitch(props: ToggleSwitchProps) {
  return (
    <button
      type="button"
      class="settings-switch"
      classList={{ "settings-switch-on": props.checked }}
      data-setting={props.setting}
      role="switch"
      aria-checked={props.checked}
      aria-label={props.label}
      disabled={props.disabled}
      onClick={props.onChange}
    >
      <span class="settings-switch-knob" />
    </button>
  );
}

interface SegmentedProps<T extends string> {
  setting: string;
  label: string;
  options: { id: T; label: string }[];
  value: T;
  onChange: (id: T) => void;
}

/**
 * A radiogroup, so the arrow keys move the choice and Tab leaves the group.
 * Only the checked cell is tabbable; that is the roving-tabindex the pattern
 * asks for.
 */
function Segmented<T extends string>(props: SegmentedProps<T>) {
  let groupRef: HTMLDivElement | undefined;

  function move(delta: number) {
    const ids = props.options.map((o) => o.id);
    const current = ids.indexOf(props.value);
    const next = ids[(current + delta + ids.length) % ids.length];
    props.onChange(next);
    requestAnimationFrame(() =>
      groupRef?.querySelector<HTMLButtonElement>(`[data-option="${next}"]`)?.focus(),
    );
  }

  function onKeyDown(event: KeyboardEvent) {
    switch (event.key) {
      case "ArrowRight":
      case "ArrowDown":
        event.preventDefault();
        move(1);
        break;
      case "ArrowLeft":
      case "ArrowUp":
        event.preventDefault();
        move(-1);
        break;
    }
  }

  return (
    <div
      ref={groupRef}
      class="settings-seg"
      role="radiogroup"
      aria-label={props.label}
      data-setting={props.setting}
      onKeyDown={onKeyDown}
    >
      <For each={props.options}>
        {(option) => (
          <button
            type="button"
            class="settings-seg-option"
            role="radio"
            data-option={option.id}
            aria-checked={props.value === option.id}
            tabindex={props.value === option.id ? 0 : -1}
            onClick={() => props.onChange(option.id)}
          >
            {option.label}
          </button>
        )}
      </For>
    </div>
  );
}

interface SettingsRowProps {
  id: string;
  label: string;
  labelFor?: string;
  /** What the row does, in the label's slot and a neutral tone. */
  description?: string;
  /** What the row warns about, in the same slot. */
  caution?: string;
  /** A control that belongs with the label rather than with the row's own
   * control, such as the link to a provider's key page. */
  labelAside?: JSX.Element;
  align?: "start";
  children: JSX.Element;
}

/** What a row's description can be attached to, in the order a row holds one. */
const ROW_CONTROL = '[role="switch"], [role="radiogroup"], input, select, textarea, button';

function SettingsRow(props: SettingsRowProps) {
  const search = useSearch();
  const descriptionId = createUniqueId();
  const cautionId = createUniqueId();
  let rowRef: HTMLDivElement | undefined;

  const describedBy = () =>
    [props.description ? descriptionId : "", props.caution ? cautionId : ""]
      .filter(Boolean)
      .join(" ");

  // The control is `children`, so the row cannot hand it a prop. It finds the
  // control inside its own element instead: the description belongs on the
  // control as a description, not inside the label, where it became part of the
  // control's accessible name.
  createEffect(() => {
    const ids = describedBy();
    if (!rowRef) return;
    // Not simply the first match: `labelAside` puts a control inside the label
    // column, ahead of the row's own in document order.
    const control = props.labelFor
      ? rowRef.querySelector<HTMLElement>(`[id="${props.labelFor}"]`)
      : (Array.from(rowRef.querySelectorAll<HTMLElement>(ROW_CONTROL)).find(
          (el) => !el.closest(".settings-row-label"),
        ) ?? null);
    if (!control) return;
    if (ids) control.setAttribute("aria-describedby", ids);
    else control.removeAttribute("aria-describedby");
  });

  return (
    <Show when={search.rowVisible(props.id)}>
      <div
        ref={rowRef}
        class="settings-row"
        classList={{ "settings-row-highlight": search.highlighted(props.id) }}
        data-setting-id={props.id}
        data-align={props.align}
      >
        <span class="settings-row-label">
          <Show
            when={props.labelFor}
            fallback={<span class="settings-row-label-text">{props.label}</span>}
          >
            <label class="settings-row-label-text" for={props.labelFor}>
              {props.label}
            </label>
          </Show>
          {props.labelAside}
          <Show when={props.description}>
            <span id={descriptionId} class="settings-row-description">
              {props.description}
            </span>
          </Show>
          <Show when={props.caution}>
            <span id={cautionId} class="settings-row-caution">
              {props.caution}
            </span>
          </Show>
        </span>
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

  function onStatusBarToggle() {
    void patchConfig((prev) => ({
      ...prev,
      editor: { ...prev.editor, status_bar: !prev.editor.status_bar },
    }));
  }

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
        <ToggleSwitch
          setting="word_wrap"
          label="Word wrap"
          checked={cfg().word_wrap}
          onChange={onWordWrapToggle}
        />
      </SettingsRow>
      <SettingsRow
        id="editor.markdown_typography"
        label="Style headings and bold text as you type"
      >
        <ToggleSwitch
          setting="markdown_typography"
          label="Style headings and bold text as you type"
          checked={cfg().markdown_typography}
          onChange={onMarkdownTypographyToggle}
        />
      </SettingsRow>
      <SettingsRow id="editor.markdown_editing" label="Markdown shortcuts">
        <ToggleSwitch
          setting="markdown_editing"
          label="Markdown shortcuts"
          checked={cfg().markdown_editing}
          onChange={onMarkdownEditingToggle}
        />
      </SettingsRow>
      <SettingsRow id="editor.status_bar" label="Status bar">
        <ToggleSwitch
          setting="status_bar"
          label="Status bar"
          checked={cfg().status_bar}
          onChange={onStatusBarToggle}
        />
      </SettingsRow>
      <SettingsRow id="editor.spelling" label="Spell check">
        <ToggleSwitch
          setting="spelling_enabled"
          label="Spell check"
          checked={spelling().enabled}
          onChange={onSpellingToggle}
        />
      </SettingsRow>
      <SettingsRow
        id="editor.spelling_dialect"
        label="Dictionary"
        labelFor="setting-spelling-dialect"
        caution={spelling().enabled ? undefined : "Turn on Spell check to pick a dictionary."}
      >
        <select
          id="setting-spelling-dialect"
          class="settings-select"
          data-setting="spelling_dialect"
          disabled={!spelling().enabled}
          value={spelling().dialect}
          onChange={(e) => onSpellingDialectChange(e.currentTarget.value)}
        >
          <option value="american">English (US)</option>
          <option value="british">English (UK)</option>
          <option value="canadian">English (Canada)</option>
          <option value="australian">English (Australia)</option>
        </select>
      </SettingsRow>
    </div>
  );
}

const REFRESH_DELAY_MS = 800;

interface DefaultAppItemProps {
  type: ClaimableType;
  status: DefaultAppStatus;
  claiming: boolean;
  onClaim: (typeId: string) => void;
}

/**
 * One file type in the default-app list.
 *
 * The box is checked and disabled once Writ holds the type: Launch Services has
 * no give-it-back call, so an enabled box would offer an action that cannot
 * run. The note under the list says where the handover is undone instead.
 */
function DefaultAppItem(props: DefaultAppItemProps) {
  const extList = () => props.type.exts.map((e) => `.${e}`).join(", ");
  const isDefault = () => props.status.status === "is_default";
  const otherName = () =>
    props.status.status === "other_app" ? (props.status.name ?? null) : null;

  return (
    <label class="settings-file-type">
      <input
        type="checkbox"
        class="settings-file-type-box"
        data-action={`make-default-${props.type.id}`}
        data-default-app-type={props.type.id}
        checked={isDefault()}
        disabled={isDefault() || props.claiming}
        aria-busy={props.claiming}
        onChange={() => props.onClaim(props.type.id)}
      />
      <span class="settings-file-type-text">
        <span class="settings-file-type-name">{props.type.label}</span>{" "}
        <span class="settings-file-type-exts">{extList()}</span>
        <Show when={otherName()}>
          {(name) => <span class="settings-file-type-owner">Now opens in {name()}</span>}
        </Show>
      </span>
    </label>
  );
}

function FilesSection() {
  const [types, setTypes] = createSignal<ClaimableType[]>([]);
  const [statuses, setStatuses] = createSignal<Readonly<Record<string, DefaultAppStatus>>>({});
  const [claimingId, setClaimingId] = createSignal<string | null>(null);
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;

  async function loadStatus(typeId: string) {
    let next: DefaultAppStatus;
    try {
      next = await fetchDefaultAppStatus(typeId);
    } catch {
      // Non-macOS or sandboxed; treat as unsupported.
      next = { status: "unsupported" };
    }
    setStatuses((prev) => ({ ...prev, [typeId]: next }));
    // Keep the shared support registry fresh as each type resolves, so search
    // headers and the command palette never offer a row that will not render.
    markDefaultAppTypeSupported(typeId, next.status !== "unsupported");
  }

  onMount(() => {
    void fetchDefaultAppTypes()
      .then((list) => {
        setTypes(list);
        return Promise.all(list.map((t) => loadStatus(t.id)));
      })
      .catch(() => setTypes([]));
  });

  onCleanup(() => {
    if (refreshTimer !== undefined) clearTimeout(refreshTimer);
  });

  function onClaim(typeId: string) {
    setClaimingId(typeId);
    void claimDefaultApp(typeId)
      .then(() => {
        // Launch Services registers asynchronously — re-query after a short
        // delay and show the handler the OS actually recorded.
        refreshTimer = setTimeout(() => {
          void loadStatus(typeId).finally(() => setClaimingId(null));
        }, REFRESH_DELAY_MS);
      })
      .catch(() => {
        const label = types().find((t) => t.id === typeId)?.label ?? typeId;
        showToast(`Could not make Writ the default for ${label}`, "error");
        // A group spans several UTIs; a mid-loop failure may have claimed some.
        // Re-query so the boxes reflect the real partial state, not a stale one.
        void loadStatus(typeId).finally(() => setClaimingId(null));
      });
  }

  const claimable = () =>
    types().filter((t) => {
      const s = statuses()[t.id];
      return s !== undefined && s.status !== "unsupported";
    });

  function onDefaultExtensionChange(raw: string) {
    const extension = raw as FileExtension;
    void patchConfig((prev) => ({ ...prev, files: { ...prev.files, default_extension: extension } }));
  }

  // The format row is every platform's, so the section is too. The file-types
  // row is the one that can be missing: the registry answers first, App probes
  // it at startup before Settings can open, and this section re-queries on
  // every visit.
  return (
    <div data-section="files">
      <SectionLabel section="files" />
      <FolderRow />
      <SettingsRow
        id="files.default_extension"
        label="Default format"
        labelFor="setting-default-extension"
      >
        <select
          id="setting-default-extension"
          class="settings-select"
          data-setting="default_extension"
          value={configStore.config().files.default_extension}
          onChange={(e) => onDefaultExtensionChange(e.currentTarget.value)}
        >
          <option value="txt">Plain text (.txt)</option>
          <option value="md">Markdown (.md)</option>
        </select>
      </SettingsRow>
      <Show when={hasSupportedDefaultAppTypes() || claimable().length > 0}>
        <SettingsRow id={DEFAULT_APP_SETTING_ID} label="Open these file types with Writ">
          <span class="settings-file-types">
            <For each={claimable()}>
              {(t) => (
                <DefaultAppItem
                  type={t}
                  status={statuses()[t.id]!}
                  claiming={claimingId() === t.id}
                  onClaim={onClaim}
                />
              )}
            </For>
            <span class="settings-file-types-note">
              Writ keeps a type until you pick another app in {FILE_MANAGER_NAME}.
            </span>
          </span>
        </SettingsRow>
      </Show>
      <CliRow />
      <VersionsRow />
    </div>
  );
}

/**
 * Says where the files went when the folder in the settings could not be used.
 *
 * The refused path is not named: it is the one the user typed or picked, it is
 * on the same row above this line, and repeating it says nothing the reader
 * does not already have.
 */
function fallbackLine(displayPath: string, reason: NotesFallbackReason): string {
  const why =
    reason === "holds_writ_data" ? "holds Writ's own data" : "could not be used";
  return `The folder in your settings ${why}, so files are in ${displayPath}.`;
}

/** Names the files a move would have written over, at most three of them. */
function collisionLine(names: string[]): string {
  const shown = names.slice(0, 3).map((name) => `“${name}”`).join(", ");
  const rest = names.length - 3;
  const list = rest > 0 ? `${shown} and ${rest} more` : shown;
  return `That folder already has ${list}. Nothing moved.`;
}

/**
 * The Files rows naming the folder Writ writes into.
 *
 * Its own component rather than inline in `FilesSection`: the move, the copy
 * and the reveal each own state and a failure path, and the row reads as one
 * thing next to the format and file-type rows.
 */
function FolderRow() {
  const folder = () => notesStore.folder();

  onMount(() => {
    void notesStore.loadFolder().catch(() => {});
  });

  async function onShow() {
    try {
      await notesStore.showInFileManager();
    } catch {
      showToast(`Could not open ${FILE_MANAGER_NAME}`, "error");
    }
  }

  async function onCopy() {
    try {
      await notesStore.copyPath();
      showToast("Path copied", "success");
    } catch {
      showToast("Could not copy the path", "error");
    }
  }

  async function onMove() {
    try {
      const outcome = await notesStore.move();
      if (!outcome) return;
      if (outcome.collided.length > 0) {
        showToast(collisionLine(outcome.collided), "error");
        return;
      }
      showToast(`Your files are now in ${folder()?.display_path ?? outcome.new_root}.`, "success");
    } catch {
      showToast("Could not move the folder", "error");
      logFailure("the folder could not be moved");
    }
  }

  return (
    <SettingsRow id="notes.folder" label="Folder">
      <span class="settings-notes">
        <span class="settings-notes-controls">
          <Tooltip label={folder()?.path ?? ""}>
            <span class="settings-notes-path" data-notes-path>
              {folder()?.display_path ?? "…"}
            </span>
          </Tooltip>
          <Button data-action="notes-show" onClick={() => void onShow()}>
            {SHOW_IN_FILE_MANAGER}
          </Button>
          <Button data-action="notes-copy" onClick={() => void onCopy()}>
            Copy path
          </Button>
          <Button data-action="notes-move" onClick={() => void onMove()}>
            Move…
          </Button>
        </span>
        <Show when={folder()?.fallback}>
          {(fallback) => (
            <span class="settings-notes-note" data-notes-fallback>
              {fallbackLine(folder()?.display_path ?? "", fallback().reason)}
            </span>
          )}
        </Show>
        <NotesSyncNote provider={folder()?.sync_provider ?? null} />
      </span>
    </SettingsRow>
  );
}

/** What Writ keeps of a file's earlier versions. */
function VersionsRow() {
  return (
    <SettingsRow id="notes.versions" label="Versions">
      <span class="settings-notes-note" data-notes-retention>
        Writ keeps a version each time a file is saved, for up to 30 days or 200 versions.
      </span>
    </SettingsRow>
  );
}

/** The Files row installing the `writ` terminal command. */
function CliRow() {
  const [isInstallingCli, setIsInstallingCli] = createSignal(false);
  const [cliInstalled, setCliInstalled] = createSignal(false);

  function refreshCliStatus() {
    void fetchCliStatus()
      .then((s) => setCliInstalled(s.installed))
      .catch(() => setCliInstalled(false));
  }

  onMount(() => {
    refreshCliStatus();
  });

  async function onInstallCli() {
    if (isInstallingCli()) return;
    setIsInstallingCli(true);
    try {
      const result = await installCli();
      setCliInstalled(true);
      showToast(`writ installed at ${result.symlink_path}`, "success");
    } catch {
      showToast("Could not install the writ command", "error");
      logFailure("the writ command could not be installed");
      refreshCliStatus();
    } finally {
      setIsInstallingCli(false);
    }
  }

  return (
    <Show when={isSettingAvailable("files.cli")}>
      <SettingsRow
        id="files.cli"
        label="Terminal command"
        caution={
          !cliInstalled() && IS_MAC
            ? "macOS will ask for your password to install this."
            : undefined
        }
      >
        <Show
          when={!cliInstalled()}
          fallback={
            <span class="settings-default-app-status settings-default-app-status-active">
              writ command installed
            </span>
          }
        >
          <Button
            data-action="install-cli"
            disabled={isInstallingCli()}
            onClick={() => void onInstallCli()}
          >
            {isInstallingCli() ? "Installing…" : "Install the writ command"}
          </Button>
        </Show>
      </SettingsRow>
    </Show>
  );
}

/** The Advanced row naming the folder Writ keeps its own files in. */
function DataFolderRow() {
  const [info, setInfo] = createSignal<StorageInfo | null>(null);

  onMount(() => {
    void fetchStorageInfo()
      .then(setInfo)
      .catch(() => setInfo(null));
  });

  async function onShow() {
    try {
      await revealStoragePath();
    } catch {
      showToast(`Could not open ${FILE_MANAGER_NAME}`, "error");
    }
  }

  async function onCopy() {
    const path = info()?.dir;
    if (!path) return;
    try {
      await copyStoragePath(path);
      showToast("Path copied", "success");
    } catch {
      showToast("Could not copy the path", "error");
    }
  }

  return (
    <SettingsRow id="storage.location" label="Writ's data folder">
      <span class="settings-inbox-controls">
        <Tooltip label={info()?.dir ?? ""}>
          <span class="settings-inbox-path" data-storage-path>
            {info()?.dir ?? "…"}
          </span>
        </Tooltip>
        <Button data-action="storage-reveal" onClick={() => void onShow()}>
          {SHOW_IN_FILE_MANAGER}
        </Button>
        <Button data-action="storage-copy" onClick={() => void onCopy()}>
          Copy path
        </Button>
      </span>
    </SettingsRow>
  );
}

function PreviewSection() {
  const cfg = () => configStore.config().preview;

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
      <SettingsRow
        id="preview.run_scripts"
        label="Allow HTML files to run their scripts"
        caution="Off is safer."
      >
        <ToggleSwitch
          setting="run_scripts"
          label="Allow HTML files to run their scripts"
          checked={cfg().run_scripts}
          onChange={onRunScriptsToggle}
        />
      </SettingsRow>
      <SettingsRow
        id="preview.layout_md"
        label="When opening a Markdown file, show"
        labelFor="setting-layout-md"
      >
        <select
          id="setting-layout-md"
          class="settings-select"
          data-setting="default_layout_markdown"
          value={cfg().default_layout_markdown}
          onChange={(e) => onDefaultLayoutMarkdownChange(e.currentTarget.value)}
        >
          <option value="source">The text</option>
          <option value="split">Text and preview</option>
          <option value="preview">Preview</option>
        </select>
      </SettingsRow>
      <SettingsRow
        id="preview.layout_html"
        label="When opening an HTML file, show"
        labelFor="setting-layout-html"
      >
        <select
          id="setting-layout-html"
          class="settings-select"
          data-setting="default_layout_html"
          value={cfg().default_layout_html}
          onChange={(e) => onDefaultLayoutHtmlChange(e.currentTarget.value)}
        >
          <option value="source">The text</option>
          <option value="split">Text and preview</option>
          <option value="preview">Preview</option>
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
        <ToggleSwitch
          setting="updater_auto_check"
          label="Check for updates automatically"
          checked={autoCheck()}
          onChange={onAutoCheckToggle}
        />
      </SettingsRow>
      <SettingsRow id="updates.check_now" label="Updates">
        <Button
          data-action="check-updates-now"
          onClick={() => void updateStore.checkForUpdate()}
        >
          Check now
        </Button>
      </SettingsRow>
    </div>
  );
}

/** Where a rewrite can be started, named for the description under its row:
 * the status-bar chip, the editor's context menu and the palette commands. */
const REWRITE_SURFACES =
  "Adds a Rewrite button to the status bar, and rewrite actions to the right-click menu and the command palette.";

const OLLAMA_DOWNLOAD_URL = "https://ollama.com/download";

/** The one-line help under a local runtime that did not answer its port. The
 * probe cannot tell "installed but stopped" from "not installed", so each line
 * states the fact it knows and, for Ollama, the link covers the other case. */
const LOCAL_HELP: Record<string, string> = {
  ollama: "Writ could not reach Ollama on port 11434.",
  lmstudio: "Writ could not reach LM Studio on port 1234. Start its server in the Developer tab.",
};

/** One connection, then the two features that use it (ADR-040 section 1).
 *
 * Every row reads the provider table `writ-core` owns, so the base URL, the
 * key page, the default model and the probe port are never spelled out here. */
function AiSection() {
  const cfg = () => configStore.config().ai;
  const [keyState, setKeyState] = createSignal<AiKeyState | null>(null);
  const [keyInput, setKeyInput] = createSignal("");
  const [keyBusy, setKeyBusy] = createSignal(false);
  const [connecting, setConnecting] = createSignal(false);
  const [probe, setProbe] = createSignal<LocalProbe | null>(null);
  // The catalog lives in the connection store, so the panel and the chat pane
  // read one list and it is stamped with the provider it was read for.
  const liveModels = (): string[] => {
    const held = aiConnectionStore.catalog();
    return held?.source === "live" ? held.models : [];
  };
  const listError = (): ModelListError | null => aiConnectionStore.catalog()?.error ?? null;
  // The user has picked (or typed) the current model this session; guards the
  // Ollama auto-select from replacing a deliberate choice.
  const [userSelected, setUserSelected] = createSignal(false);
  // Free-text model entry is showing (chose "Custom…", or the model is not in
  // the offered list).
  const [customMode, setCustomMode] = createSignal(false);
  // The chat is set to a model of its own. Opening the disclosure seeds it, so
  // the select always shows the model chat would actually use.
  const [chatModelOpen, setChatModelOpen] = createSignal(cfg().chat.model !== "");

  const provider = () => aiProvidersStore.byId(cfg().provider);
  const isLocal = () => provider()?.group === "local";
  const isCustom = () => cfg().provider === "custom";
  const needsKey = () => provider()?.needs_key === true || (isCustom() && !isLocal());
  const keyPageUrl = () => provider()?.key_page_url ?? null;

  void aiProvidersStore.load();

  /** Whether the selected local runtime answered. Unknown until the probe
   * lands, which is what keeps the pill off a row that was never probed. */
  const localRunning = (): boolean | null => {
    const p = probe();
    if (!p || !isLocal()) return null;
    return cfg().provider === "ollama" ? p.ollama : p.lmstudio;
  };

  async function refreshProbe(): Promise<void> {
    try {
      setProbe(await aiConnectionStore.probeLocal());
    } catch {
      setProbe(null);
    }
  }

  async function refreshModels(): Promise<void> {
    await aiConnectionStore.refreshCatalog();
  }

  // The table, the probe and the model list, once the section can be seen and
  // again whenever the provider changes. The probe answers both local rows at
  // once, so the pill on either is one request.
  createEffect(() => {
    void cfg().provider;
    void aiProvidersStore.load();
    void refreshProbe();
    void refreshModels();
  });

  // The key belongs to the provider, so reading it follows the provider.
  createEffect(() => {
    const id = cfg().provider;
    void aiRewriteStore
      .hasApiKey(id)
      .then(setKeyState)
      .catch(() => setKeyState(null));
  });

  // Check the connection when the section opens and whenever the target
  // changes, debounced so typing a URL does not fire a request per keystroke.
  createEffect(() => {
    // Track the fields that define the target. `consented_hosts` is one of
    // them: a hosted endpoint is not reached before consent, so granting it is
    // what makes the next check possible.
    void cfg().provider;
    void cfg().base_url;
    void cfg().model;
    void cfg().consented_hosts;
    aiConnectionStore.scheduleCheck();
  });

  const connection = () => {
    const failed = listError();
    if (failed && !aiConnectionStore.status()) {
      return modelListDisplay(failed, endpointHost());
    }
    return connectionDisplay(aiConnectionStore.status(), cfg().model);
  };

  const modelOptionList = () => modelOptions(cfg().provider, liveModels());
  const usingCurated = () => liveModels().length === 0;
  const modelSelectValue = () => {
    if (customMode()) return "__custom__";
    return modelOptionList().includes(cfg().model) ? cfg().model : "__custom__";
  };
  const showModelInput = () => customMode() || !modelOptionList().includes(cfg().model);
  const modelOptionLabel = (id: string) => (usingCurated() ? `${id} (suggested)` : id);

  // Keep a working model without a setup decision: fill an empty one, and for a
  // local server that has just listed its models, switch off a not-installed id.
  createEffect(() => {
    const auto = resolveAutoModel({
      provider: cfg().provider,
      model: cfg().model,
      live: liveModels(),
      userSelected: userSelected(),
    });
    if (auto && auto !== cfg().model) {
      void patchConfig((prev) => ({ ...prev, ai: { ...prev.ai, model: auto } }));
    }
  });

  // Where the endpoint points and what it still needs. Resolved in Rust by the
  // same code the request guard uses, so this panel can never disagree with the
  // guard about which host consent applies to.
  const [endpoint, setEndpoint] = createSignal<AiEndpointState | null>(null);

  createEffect(() => {
    // Track the fields that change the answer.
    void cfg().base_url;
    void cfg().provider;
    void cfg().consented_hosts;
    void aiRewriteStore
      .endpointState()
      .then(setEndpoint)
      .catch(() => setEndpoint(null));
  });

  const endpointHost = () => {
    const e = endpoint();
    return e?.host_port ?? e?.host ?? "";
  };

  // Hosted (non-local) and not yet consented to — shown for a provider switch
  // or a hand-edited base URL alike.
  const hostedUnconsented = () => {
    const e = endpoint();
    return Boolean(e?.is_allowed && e.is_hosted && !e.is_consented);
  };

  const showLocalLine = () => Boolean(isLocal() && endpointHost());

  function seedProvider(id: string) {
    // A new provider is a fresh context: reset the selection guard, and let
    // the connection store make the change. Rust seeds the row's model and
    // drops a chat model that belonged to the old provider, so the panel and
    // the pane cannot clear an override differently.
    setUserSelected(false);
    setCustomMode(id === "custom");
    setChatModelOpen(false);
    void aiConnectionStore.selectProvider(id).catch(() => {
      showToast("Could not save your settings", "error");
    });
  }

  function onProviderChange(raw: string) {
    seedProvider(raw);
  }

  function onBaseUrlChange(raw: string) {
    void patchConfig((prev) => ({ ...prev, ai: { ...prev.ai, base_url: raw.trim() } }));
  }

  function onModelText(raw: string) {
    setUserSelected(true);
    void patchConfig((prev) => ({ ...prev, ai: { ...prev.ai, model: raw.trim() } }));
  }

  function onSelectModel(value: string) {
    if (value === "__custom__") {
      setCustomMode(true);
      return;
    }
    setCustomMode(false);
    setUserSelected(true);
    void patchConfig((prev) => ({ ...prev, ai: { ...prev.ai, model: value } }));
  }

  function patchChat(next: Partial<{ enabled: boolean }>) {
    void patchConfig((prev) => ({
      ...prev,
      ai: { ...prev.ai, chat: { ...prev.ai.chat, ...next } },
    }));
  }

  function onChatModelDisclosure() {
    if (chatModelOpen()) {
      setChatModelOpen(false);
      void aiConnectionStore.selectChatModel(null);
      return;
    }
    setChatModelOpen(true);
    void aiConnectionStore.selectChatModel(cfg().model || modelOptionList()[0] || "");
  }

  // Consent is recorded host-side: the command resolves the host itself and
  // persists it, so the stored value is exactly what the guard checks.
  async function onConsent() {
    try {
      setEndpoint(await aiRewriteStore.consentHost());
      await configStore.load();
      await refreshModels();
    } catch {
      showToast("Could not record the choice", "error");
    }
  }

  async function onSetKey() {
    const key = keyInput();
    if (!key || keyBusy()) return;
    setKeyBusy(true);
    try {
      const state = await aiRewriteStore.setApiKey(cfg().provider, key);
      setKeyState(state);
      setKeyInput("");
      await refreshModels();
      if (state.memory_only) {
        showToast("Writ could not save your key, so you will enter it again next time", "info");
      }
    } catch {
      showToast("Could not save the API key", "error");
    } finally {
      setKeyBusy(false);
    }
  }

  async function onClearKey() {
    if (keyBusy()) return;
    setKeyBusy(true);
    try {
      const state = await aiRewriteStore.clearApiKey(cfg().provider);
      setKeyState(state);
    } catch {
      showToast("Could not clear the API key", "error");
    } finally {
      setKeyBusy(false);
    }
  }

  async function onConnect() {
    if (connecting()) {
      void aiConnectionStore.cancelOpenrouter();
      return;
    }
    setConnecting(true);
    try {
      setKeyState(await aiConnectionStore.connectOpenrouter());
      await refreshModels();
    } catch (err) {
      showToast(typeof err === "string" ? err : "The key exchange failed.", "error");
    } finally {
      setConnecting(false);
    }
  }

  return (
    <div data-section="ai">
      <SectionLabel section="ai" />

      <SettingsRow
        id="ai.provider"
        label="Provider"
        description="The service that runs the model."
        labelFor="setting-ai-provider"
      >
        <span class="settings-provider-control">
          <select
            id="setting-ai-provider"
            class="settings-select"
            data-setting="ai_provider"
            value={cfg().provider}
            onChange={(e) => onProviderChange(e.currentTarget.value)}
          >
            <For each={aiProvidersStore.grouped()}>
              {(group) => (
                <optgroup label={group.label}>
                  <For each={group.providers}>
                    {/* The table loads after this mounts, so the option carries the
                        choice: `value` on the select alone leaves the first showing. */}
                    {(row) => (
                      <option value={row.id} selected={row.id === cfg().provider}>
                        {row.label}
                      </option>
                    )}
                  </For>
                </optgroup>
              )}
            </For>
          </select>
          <Show when={localRunning() !== null}>
            <span class="settings-ai-pill" data-running={localRunning() ? "yes" : "no"}>
              {localRunning() ? "Running" : "Not running"}
            </span>
          </Show>
        </span>
      </SettingsRow>

      <Show when={localRunning() === false}>
        <div class="settings-ai-note">
          {LOCAL_HELP[cfg().provider]}
          <Show when={cfg().provider === "ollama"}>
            {" "}
            <button
              type="button"
              class="settings-ai-link"
              data-action="get-ollama"
              onClick={() => void linkStore.openExternal(OLLAMA_DOWNLOAD_URL)}
            >
              Get Ollama
            </button>
          </Show>
        </div>
      </Show>

      <Show when={isCustom()}>
        <SettingsRow
          id="ai.base_url"
          label="Base URL"
          description="Most servers expect /v1 at the end."
          labelFor="setting-ai-base-url"
        >
          <input
            id="setting-ai-base-url"
            type="text"
            class="settings-input"
            data-setting="ai_base_url"
            spellcheck={false}
            autocomplete="off"
            placeholder="https://api.example.com/v1"
            value={cfg().base_url}
            onChange={(e) => onBaseUrlChange(e.currentTarget.value)}
          />
        </SettingsRow>
      </Show>

      <Show when={hostedUnconsented()}>
        <div class="settings-ai-consent" role="note">
          <p class="settings-ai-consent-text">
            The files you attach and the text you rewrite are sent to {endpointHost()} with your API
            key. Writ also sends the key on its own to check the host is reachable; nothing else
            leaves your machine.
          </p>
          <Button data-action="ai-consent" onClick={() => void onConsent()}>
            Allow
          </Button>
        </div>
      </Show>

      <Show when={showLocalLine()}>
        <div class="settings-ai-note" data-note="local-endpoint">
          Requests go to {endpointHost()} on this machine. Nothing leaves it.
        </div>
      </Show>

      <Show when={needsKey()}>
        <SettingsRow
          id="ai.api_key"
          label="API key"
          description="Kept in your system keychain."
          labelAside={
            <Show when={keyPageUrl()}>
              <button
                type="button"
                class="settings-ai-link"
                data-action="ai-key-page"
                onClick={() => void linkStore.openExternal(keyPageUrl()!)}
              >
                Get a key
              </button>
            </Show>
          }
        >
          <span class="settings-inbox-controls">
            <input
              type="password"
              class="settings-input"
              data-setting="ai_api_key"
              spellcheck={false}
              autocomplete="off"
              placeholder={keyState()?.is_set ? "Key set" : "Not set"}
              value={keyInput()}
              onInput={(e) => setKeyInput(e.currentTarget.value)}
            />
            <Button
              data-action="ai-set-key"
              disabled={keyBusy() || keyInput().length === 0}
              onClick={() => void onSetKey()}
            >
              Save
            </Button>
            <Show when={keyState()?.is_set}>
              <Button
                data-action="ai-clear-key"
                disabled={keyBusy()}
                onClick={() => void onClearKey()}
              >
                Clear
              </Button>
            </Show>
            <Show when={provider()?.supports_connect}>
              <Button data-action="ai-connect" onClick={() => void onConnect()}>
                {connecting() ? "Cancel" : "Connect"}
              </Button>
            </Show>
          </span>
        </SettingsRow>
      </Show>
      <Show when={keyState()?.is_set && keyState()?.memory_only}>
        <div class="settings-ai-note">
          Your key is not saved. You will enter it again next time you open Writ.
        </div>
      </Show>

      <SettingsRow
        id="ai.model"
        label="Model"
        description="Used for rewriting and chat."
        labelFor="setting-ai-model"
      >
        <div class="settings-model-picker">
          <select
            id="setting-ai-model"
            class="settings-select"
            data-setting="ai_model"
            value={modelSelectValue()}
            onChange={(e) => onSelectModel(e.currentTarget.value)}
          >
            <For each={modelOptionList()}>
              {(id) => <option value={id}>{modelOptionLabel(id)}</option>}
            </For>
            <option value="__custom__">Custom…</option>
          </select>
          <Show when={showModelInput()}>
            <input
              type="text"
              class="settings-input"
              data-setting="ai_model_custom"
              spellcheck={false}
              autocomplete="off"
              placeholder="Model id"
              value={cfg().model}
              onChange={(e) => onModelText(e.currentTarget.value)}
            />
          </Show>
        </div>
      </SettingsRow>
      <Show when={keyState()?.is_set && !cfg().model.trim()}>
        <div class="settings-ai-note" data-note="choose-a-model">
          {CHOOSE_A_MODEL}
        </div>
      </Show>

      <SettingsRow
        id="ai.connection"
        label="Connection"
        description="Asks the server for its model list."
      >
        <span class="settings-ai-connection">
          <span
            class="settings-ai-connection-status"
            data-tone={connection().tone}
            aria-live="polite"
          >
            {connection().text}
          </span>
          <Button
            data-action="ai-recheck"
            disabled={aiConnectionStore.checking()}
            onClick={() => void aiConnectionStore.check()}
          >
            {aiConnectionStore.checking() ? "Checking…" : "Check"}
          </Button>
        </span>
      </SettingsRow>

      <div class="settings-group">
        <SettingsRow
          id="ai.rewrite.enabled"
          label="Rewrite selected text"
          description={REWRITE_SURFACES}
        >
          <ToggleSwitch
            setting="ai_rewrite_enabled"
            label="Rewrite selected text"
            checked={cfg().rewrite.enabled}
            onChange={() => patchRewrite(!cfg().rewrite.enabled)}
          />
        </SettingsRow>
      </div>

      <div class="settings-group">
        <SettingsRow
          id="ai.chat.enabled"
          label="Chat about your files"
          description="A pane where you attach files and the model can offer changes you apply."
        >
          <ToggleSwitch
            setting="ai_chat_enabled"
            label="Chat about your files"
            checked={cfg().chat.enabled}
            onChange={() => patchChat({ enabled: !cfg().chat.enabled })}
          />
        </SettingsRow>

        <SettingsRow
          id="ai.chat.model"
          label="Use a different model for chat"
          description="Off means chat uses the model above."
        >
          <span class="settings-model-picker">
            <ToggleSwitch
              setting="ai_chat_model_disclosure"
              label="Use a different model for chat"
              checked={chatModelOpen()}
              onChange={onChatModelDisclosure}
            />
            <Show when={chatModelOpen()}>
              <select
                class="settings-select"
                data-setting="ai_chat_model"
                aria-label="Chat model"
                value={cfg().chat.model}
                onChange={(e) => void aiConnectionStore.selectChatModel(e.currentTarget.value)}
              >
                {/* The catalog arrives after this mounts and `chat.model` does not
                    change with it, so the option carries the choice. */}
                <For each={modelOptionList()}>
                  {(id) => (
                    <option value={id} selected={id === cfg().chat.model}>
                      {modelOptionLabel(id)}
                    </option>
                  )}
                </For>
              </select>
            </Show>
          </span>
        </SettingsRow>
      </div>
    </div>
  );
}

function patchRewrite(enabled: boolean) {
  void patchConfig((prev) => ({
    ...prev,
    ai: { ...prev.ai, rewrite: { ...prev.ai.rewrite, enabled } },
  }));
}

const POLARITY_OPTIONS: { id: Polarity; label: string }[] = [
  { id: "system", label: "System" },
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
];

const ACCENT_OPTIONS: { id: AccentId; label: string }[] = [
  { id: "pine", label: "Pine" },
  { id: "writ-blue", label: "Writ blue" },
  { id: "terracotta", label: "Terracotta" },
  { id: "slate", label: "Slate" },
  { id: "plum", label: "Plum" },
  { id: "gold", label: "Gold" },
];

const PROSE_FACE_OPTIONS: { id: ProseFaceId; label: string }[] = [
  { id: "system", label: "System" },
  { id: "quattro", label: "iA Writer Quattro S" },
];

function AppearanceSection() {
  // Not the stored preset: polarity picks which half of a pair renders, and a
  // row that named the stored half read "Writ Light" over a dark app.
  const renderedPreset = () => themeStore.activePreset().id;
  const appearance = () => configStore.config().appearance;

  function onPresetChange(id: string) {
    themeStore.setPreset(id);
    // The preset pins the polarity it belongs to, so the row that renders and
    // the config on disk agree. Both go out in the same write.
    const polarity = themeStore.polarity();
    void patchConfig((prev) => ({
      ...prev,
      theme: { ...prev.theme, preset: id },
      appearance: { ...prev.appearance, polarity },
    }));
  }

  function patchAppearance(patch: Partial<AppearanceConfig>) {
    const next = { ...appearance(), ...patch };
    themeStore.setAppearance(next);
    void patchConfig((prev) => ({ ...prev, appearance: next }));
  }

  // Unset means the platform's own size, which is what the stylesheet resolves;
  // the row shows that number rather than an empty field.
  const interfaceTextSize = () =>
    appearance().interface_text_size ??
    Math.round(parseFloat(TYPE[resolvePlatform()].ui.md.size));

  function onInterfaceTextSizeChange(raw: string) {
    const value = clampInterfaceTextSize(parseIntSafe(raw, interfaceTextSize()));
    patchAppearance({ interface_text_size: value });
  }

  // One of six, so the arrows move the choice and Tab leaves the set — the
  // pattern Segmented already implements two rows above.
  let accentsRef: HTMLDivElement | undefined;

  function moveAccent(delta: number) {
    if (!themeStore.accentApplies()) return;
    const ids = ACCENT_OPTIONS.map((option) => option.id);
    const current = ids.indexOf(appearance().accent);
    const next = ids[(current + delta + ids.length) % ids.length];
    patchAppearance({ accent: next });
    requestAnimationFrame(() =>
      accentsRef?.querySelector<HTMLButtonElement>(`[data-accent="${next}"]`)?.focus(),
    );
  }

  function onAccentKeyDown(event: KeyboardEvent) {
    switch (event.key) {
      case "ArrowRight":
      case "ArrowDown":
        event.preventDefault();
        moveAccent(1);
        break;
      case "ArrowLeft":
      case "ArrowUp":
        event.preventDefault();
        moveAccent(-1);
        break;
    }
  }

  return (
    <div data-section="appearance">
      <SectionLabel section="appearance" />
      <SettingsRow id="appearance.polarity" label="Light and dark">
        <Segmented
          setting="appearance_polarity"
          label="Light and dark"
          options={POLARITY_OPTIONS}
          value={appearance().polarity}
          onChange={(polarity) => patchAppearance({ polarity })}
        />
      </SettingsRow>
      <SettingsRow
        id="appearance.accent"
        label="Accent color"
        caution={themeStore.accentApplies() ? undefined : "The current theme sets its own accent."}
      >
        <div
          ref={accentsRef}
          class="settings-accents"
          role="radiogroup"
          aria-label="Accent color"
          data-setting="appearance_accent"
          onKeyDown={onAccentKeyDown}
        >
          <For each={ACCENT_OPTIONS}>
            {(option) => (
              <button
                type="button"
                class="settings-accent"
                data-accent={option.id}
                role="radio"
                disabled={!themeStore.accentApplies()}
                aria-checked={appearance().accent === option.id}
                tabindex={appearance().accent === option.id ? 0 : -1}
                aria-label={option.label}
                onClick={() => patchAppearance({ accent: option.id })}
              >
                {/* The swatch has to show its own colour, so the fill comes from
                    the generated map rather than from --writ-accent, which is
                    whichever one is already chosen. */}
                <span style={{ background: ACCENTS[option.id][themeStore.polarity()].base }} />
              </button>
            )}
          </For>
        </div>
      </SettingsRow>
      <SettingsRow
        id="appearance.prose_face"
        label="Prose typeface"
        labelFor="setting-appearance-prose-face"
      >
        <select
          id="setting-appearance-prose-face"
          class="settings-select"
          data-setting="appearance_prose_face"
          value={appearance().prose_face}
          onChange={(e) => patchAppearance({ prose_face: e.currentTarget.value as ProseFaceId })}
        >
          {PROSE_FACE_OPTIONS.map((option) => (
            <option value={option.id}>{option.label}</option>
          ))}
        </select>
      </SettingsRow>
      <SettingsRow
        id="appearance.interface_text_size"
        label="Interface text size"
        labelFor="setting-interface-text-size"
      >
        <span class="settings-inbox-controls">
          <input
            id="setting-interface-text-size"
            type="number"
            class="settings-input settings-input-number"
            data-setting="interface_text_size"
            value={interfaceTextSize()}
            min={INTERFACE_TEXT_MIN}
            max={INTERFACE_TEXT_MAX}
            onChange={(e) => onInterfaceTextSizeChange(e.currentTarget.value)}
          />
          <span class="settings-unit">px</span>
        </span>
      </SettingsRow>
      <SettingsRow id="appearance.theme" label="Theme" labelFor="setting-theme-preset">
        <select
          id="setting-theme-preset"
          class="settings-select"
          data-setting="theme_preset"
          value={renderedPreset()}
          onChange={(e) => onPresetChange(e.currentTarget.value)}
        >
          {PRESETS.map((p) => (
            <option value={p.id}>{p.name}</option>
          ))}
        </select>
      </SettingsRow>
      <SettingsRow id="appearance.custom_colors" label="Custom colors">
        <Button
          data-action="open-theme-editor"
          onClick={() => {
            closeSettings();
            openThemeEditor();
          }}
        >
          Edit theme…
        </Button>
      </SettingsRow>
    </div>
  );
}

const SIDEBAR_ROWS: { id: string; section: SidebarSectionId; title: string; setting: string }[] = [
  { id: "sidebar.folder", section: "folder", title: "Show files", setting: "sidebar_folder" },
  { id: "sidebar.tags", section: "tags", title: "Show tags", setting: "sidebar_tags" },
  { id: "sidebar.inbox", section: "inbox", title: "Show watched folder", setting: "sidebar_inbox" },
  { id: "sidebar.recent", section: "recent", title: "Show recently closed", setting: "sidebar_recent" },
];

function SidebarSettingsSection() {
  return (
    <div data-section="sidebar">
      <SectionLabel section="sidebar" />
      <For each={SIDEBAR_ROWS}>
        {(row) => (
          <SettingsRow id={row.id} label={row.title}>
            <ToggleSwitch
              setting={row.setting}
              label={row.title}
              checked={!configStore.isSidebarSectionHidden(row.section)}
              onChange={() =>
                configStore.setSidebarSectionHidden(
                  row.section,
                  !configStore.isSidebarSectionHidden(row.section),
                )
              }
            />
          </SettingsRow>
        )}
      </For>
    </div>
  );
}

function ShortcutsSection() {
  return (
    <div data-section="shortcuts">
      <SectionLabel section="shortcuts" />
      <SettingsRow id="shortcuts.edit" label="Keyboard shortcuts">
        <Button
          data-action="open-shortcut-editor"
          onClick={() => {
            closeSettings();
            openShortcutEditor();
          }}
        >
          Edit shortcuts…
        </Button>
      </SettingsRow>
    </div>
  );
}

/**
 * The rows a writer never needs: the watched folder, the two preview size
 * limits and Writ's own data folder. Everything here answers a question the
 * panel above it does not raise.
 */
function AdvancedSection() {
  const preview = () => configStore.config().preview;
  const watchedPath = () => inboxStore.path();
  const focusOnArrival = () => configStore.config().inbox.focus;

  function onFocusToggle() {
    void patchConfig((prev) => ({ ...prev, inbox: { ...prev.inbox, focus: !prev.inbox.focus } }));
  }

  // A live threshold above the refuse threshold describes a file Writ renders
  // live and refuses to render, so each value is held under the other.
  function onLiveLimitChange(raw: string) {
    const ceiling = Math.min(100, preview().render_refuse_threshold_mb);
    const value = clamp(parseFloatSafe(raw, preview().live_render_threshold_mb), 0.1, ceiling);
    void patchConfig((prev) => ({
      ...prev,
      preview: { ...prev.preview, live_render_threshold_mb: value },
    }));
  }

  function onHardLimitChange(raw: string) {
    const value = clamp(parseFloatSafe(raw, preview().render_refuse_threshold_mb), 1, 500);
    void patchConfig((prev) => ({
      ...prev,
      preview: {
        ...prev.preview,
        render_refuse_threshold_mb: value,
        live_render_threshold_mb: Math.min(prev.preview.live_render_threshold_mb, value),
      },
    }));
  }

  return (
    <div data-section="advanced">
      <SectionLabel section="advanced" />
      <SettingsRow id="files.inbox_folder" label="Folder to watch for new files">
        <Show
          when={watchedPath()}
          fallback={
            <Button data-action="inbox-watch" onClick={() => void inboxStore.watchFolder()}>
              Choose folder…
            </Button>
          }
        >
          {(path) => (
            <span class="settings-inbox-controls">
              <Tooltip label={path()}>
                <span class="settings-inbox-path">{path()}</span>
              </Tooltip>
              <Button data-action="inbox-change" onClick={() => void inboxStore.watchFolder()}>
                Change…
              </Button>
              <Button data-action="inbox-clear" onClick={() => void inboxStore.stopWatching()}>
                Stop watching
              </Button>
            </span>
          )}
        </Show>
      </SettingsRow>
      <SettingsRow
        id="files.inbox_focus"
        label="Bring Writ to the front when a new file arrives"
      >
        <ToggleSwitch
          setting="inbox_focus"
          label="Bring Writ to the front when a new file arrives"
          checked={focusOnArrival()}
          onChange={onFocusToggle}
        />
      </SettingsRow>
      <SettingsRow
        id="preview.live_threshold"
        label="Stop live preview above"
        labelFor="setting-live-limit"
        description="Writ keeps this under the size it will not preview."
      >
        <span class="settings-inbox-controls">
          <input
            id="setting-live-limit"
            type="number"
            class="settings-input settings-input-number"
            data-setting="live_render_threshold_mb"
            value={preview().live_render_threshold_mb}
            min={0.1}
            max={100}
            step={0.5}
            onChange={(e) => onLiveLimitChange(e.currentTarget.value)}
          />
          <span class="settings-unit">MB</span>
        </span>
      </SettingsRow>
      <SettingsRow
        id="preview.refuse_threshold"
        label="Do not preview files above"
        labelFor="setting-hard-limit"
      >
        <span class="settings-inbox-controls">
          <input
            id="setting-hard-limit"
            type="number"
            class="settings-input settings-input-number"
            data-setting="render_refuse_threshold_mb"
            value={preview().render_refuse_threshold_mb}
            min={1}
            max={500}
            step={1}
            onChange={(e) => onHardLimitChange(e.currentTarget.value)}
          />
          <span class="settings-unit">MB</span>
        </span>
      </SettingsRow>
      <DataFolderRow />
    </div>
  );
}

export type ToolPhrase =
  | { group: "do"; phrase: string }
  | { group: "see"; phrase: string }
  | { group: "write"; phrase: string };

export const TOOL_PHRASES: Readonly<Record<string, ToolPhrase>> = {
  list_notes: { group: "do", phrase: "list" },
  search_notes: { group: "do", phrase: "search" },
  read_note: { group: "do", phrase: "open" },
  note_links: { group: "see", phrase: "links" },
  note_backlinks: { group: "see", phrase: "links" },
  note_properties: { group: "see", phrase: "properties" },
  note_tags: { group: "see", phrase: "tags" },
  folder_tags: { group: "see", phrase: "tags" },
  write_note: { group: "write", phrase: "replace a file's text" },
  create_note: { group: "write", phrase: "make a new file" },
  rename_note: { group: "write", phrase: "rename a file" },
};

export function describeReadTools(ids: readonly string[]): string {
  const verbs: string[] = [];
  const nouns: string[] = [];
  const unnamed: string[] = [];
  for (const id of ids) {
    const named = TOOL_PHRASES[id];
    if (named?.group === "do") {
      if (!verbs.includes(named.phrase)) verbs.push(named.phrase);
    } else if (named?.group === "see") {
      if (!nouns.includes(named.phrase)) nouns.push(named.phrase);
    } else {
      unnamed.push(id);
    }
  }
  const clauses: string[] = [];
  if (verbs.length > 0) clauses.push(`${joinAnd(verbs)} files`);
  if (nouns.length > 0) clauses.push(`see their ${joinAnd(nouns)}`);
  return [...clauses, ...unnamed].join(", and ");
}

export function describeWriteTools(ids: readonly string[]): string {
  return ids
    .map((id) => {
      const named = TOOL_PHRASES[id];
      return named?.group === "write" ? named.phrase : id;
    })
    .join(", ");
}

/**
 * Which programs may reach the notes folder, and how one is pointed at it.
 *
 * Turning the switch on approves nobody: a program's first call waits for the
 * user to decide on it, in Activity (ADR-031 rules 3.2 and 7.2). Reading and
 * writing are separate grants, so a program approved to read never writes.
 *
 * The grants read as sentences: the server reports the tool ids it registers,
 * and TOOL_PHRASES turns them into the words under each grant.
 */
function ProgramsSection() {
  const mcp = () => configStore.config().mcp;

  onMount(() => {
    void activityStore.refreshClients();
    void activityStore.loadCommand();
    void activityStore.loadTools();
  });

  function onEnableToggle() {
    void patchConfig((prev) => ({ ...prev, mcp: { ...prev.mcp, enabled: !prev.mcp.enabled } }));
  }

  async function onCopyCommand() {
    try {
      await activityStore.copyCommand();
      showToast("Command copied", "success");
    } catch {
      showToast("Could not copy the command", "error");
    }
  }

  async function onSetPermission(name: string, read: boolean, write: boolean) {
    try {
      await activityStore.setPermission(name, read, write);
    } catch {
      showToast("Could not save the change", "error");
    }
  }

  async function onForget(name: string) {
    try {
      await activityStore.forget(name);
    } catch {
      showToast("Could not forget the program", "error");
    }
  }

  return (
    <div data-section="programs">
      <SectionLabel section="programs" />

      <SettingsRow id="mcp.enabled" label="Let other programs read and write your files">
        <ToggleSwitch
          setting="mcp_enabled"
          label="Let other programs read and write your files"
          checked={mcp().enabled}
          onChange={onEnableToggle}
        />
      </SettingsRow>

      <SettingsRow id="mcp.command" label="Command to give a program">
        <span class="settings-inbox-controls">
          <Tooltip label={activityStore.serverCommand()?.command ?? ""}>
            <span class="settings-inbox-path" data-mcp-command>
              {activityStore.serverCommand()?.command ?? "…"}
            </span>
          </Tooltip>
          <Button data-action="mcp-copy-command" onClick={() => void onCopyCommand()}>
            Copy command
          </Button>
        </span>
      </SettingsRow>

      <SettingsRow
        id="mcp.tools"
        label="What a program can do"
        align="start"
        caution="Nothing deletes a file, and a rename leaves other files pointing at the old name."
      >
        <Show
          when={activityStore.tools()}
          fallback={<span class="settings-programs-none">…</span>}
        >
          {(tools) => (
            <ul class="settings-tools">
              <li class="settings-tool-grant" data-grant="read">
                <span class="settings-tool-grant-name">Reading:</span>{" "}
                <span class="settings-tool-names">{describeReadTools(tools().read)}.</span>
              </li>
              <li class="settings-tool-grant" data-grant="write">
                <span class="settings-tool-grant-name">Writing:</span>{" "}
                <span class="settings-tool-names">{describeWriteTools(tools().write)}.</span>
              </li>
            </ul>
          )}
        </Show>
      </SettingsRow>

      <SettingsRow id="mcp.clients" label="Programs you approved" align="start">
        <Show
          when={activityStore.clients().length > 0}
          fallback={<span class="settings-programs-none">None yet.</span>}
        >
          <ul class="settings-programs">
            <For each={activityStore.clients()}>
              {(client) => (
                <li class="settings-program" data-program={client.name}>
                  <div class="settings-program-row">
                    <span class="settings-program-name">{client.name}</span>
                    <span class="settings-program-grants">
                      <label class="settings-program-grant">
                        Read
                        <ToggleSwitch
                          setting={`mcp_read_${client.name}`}
                          label={`Let ${client.name} read your files`}
                          checked={client.read || client.write}
                          disabled={client.write}
                          onChange={() => void onSetPermission(client.name, !client.read, false)}
                        />
                      </label>
                      <label class="settings-program-grant">
                        Write
                        <ToggleSwitch
                          setting={`mcp_write_${client.name}`}
                          label={`Let ${client.name} write your files`}
                          checked={client.write}
                          onChange={() => void onSetPermission(client.name, true, !client.write)}
                        />
                      </label>
                      <Button
                        data-action="mcp-forget"
                        onClick={() => void onForget(client.name)}
                      >
                        Forget
                      </Button>
                    </span>
                  </div>
                  <Show when={client.write}>
                    <span class="settings-program-note" data-program-note="write">
                      Writing includes reading.
                    </span>
                  </Show>
                </li>
              )}
            </For>
          </ul>
          <span class="settings-program-note" data-program-note="forget">
            Forget removes a program. It can ask again next time it connects.
          </span>
        </Show>
      </SettingsRow>

      <SettingsRow id="mcp.activity" label="Recent activity">
        <Button
          data-action="mcp-open-activity"
          onClick={() => {
            closeSettings();
            openActivity();
          }}
        >
          Open activity…
        </Button>
      </SettingsRow>
    </div>
  );
}

function AllSections() {
  return (
    <>
      <FilesSection />
      <EditorSection />
      <PreviewSection />
      <AiSection />
      <ProgramsSection />
      <AppearanceSection />
      <SidebarSettingsSection />
      <UpdatesSection />
      <ShortcutsSection />
      <AdvancedSection />
    </>
  );
}

export default function SettingsModal() {
  const win = useWindow();
  let modalRef: HTMLDivElement | undefined;
  let contentRef: HTMLDivElement | undefined;
  let searchRef: HTMLInputElement | undefined;
  let navRef: HTMLDivElement | undefined;
  const titleId = "settings-modal-title";
  const tabId = "settings-section-tab";
  const panelId = "settings-section-panel";

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

  // Focus follows the selection: roving tabindex has just set every other tab
  // to -1, so a tab that keeps focus without keeping the selection is one Tab
  // can no longer come back to.
  function selectSection(next: SettingsSection) {
    setActiveSection(next);
    requestAnimationFrame(() =>
      navRef?.querySelector<HTMLButtonElement>(`[data-section-tab="${next}"]`)?.focus(),
    );
  }

  function moveSection(delta: number) {
    const ids = NAV_ITEMS.map((item) => item.id);
    const current = ids.indexOf(activeSection());
    selectSection(ids[(current + delta + ids.length) % ids.length]);
  }

  function onNavKeyDown(event: KeyboardEvent) {
    switch (event.key) {
      case "ArrowDown":
      case "ArrowRight":
        event.preventDefault();
        moveSection(1);
        break;
      case "ArrowUp":
      case "ArrowLeft":
        event.preventDefault();
        moveSection(-1);
        break;
      case "Home":
        event.preventDefault();
        selectSection(NAV_ITEMS[0].id);
        break;
      case "End":
        event.preventDefault();
        selectSection(NAV_ITEMS[NAV_ITEMS.length - 1].id);
        break;
    }
  }

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
              <Tooltip label="Close settings">
                <Button
                  variant="ghost"
                  icon="x"
                  iconSize={16}
                  onClick={closeSettings}
                  aria-label="Close settings"
                />
              </Tooltip>
            </div>

            <div class="settings-search-bar">
              <Icon name="magnifying-glass" class="settings-search-icon" size={16} />
              <input
                ref={searchRef}
                type="text"
                class="settings-search-input"
                data-writ-focus-silent
                placeholder="Search settings"
                value={query()}
                onInput={(e) => setQuery(e.currentTarget.value)}
                aria-label="Search settings"
              />
              <Show when={isSearching()}>
                <Tooltip label="Clear search">
                  <Button
                    variant="ghost"
                    icon="x"
                    iconSize={14}
                    onClick={() => {
                      setQuery("");
                      searchRef?.focus();
                    }}
                    aria-label="Clear search"
                  />
                </Tooltip>
              </Show>
            </div>

            <div class="settings-body">
              {/* The rail stays mounted while searching, dimmed: unmounting it
                  moved the content column 156px mid-keystroke and back on
                  clear. */}
              <div
                ref={navRef}
                class="settings-nav"
                classList={{ "settings-nav-dimmed": isSearching() }}
                role="tablist"
                aria-orientation="vertical"
                aria-label="Settings sections"
                aria-disabled={isSearching() ? "true" : undefined}
                onKeyDown={onNavKeyDown}
              >
                {NAV_ITEMS.map((item) => (
                  <button
                    type="button"
                    id={`${tabId}-${item.id}`}
                    class="settings-nav-item"
                    classList={{ "settings-nav-item-active": activeSection() === item.id }}
                    role="tab"
                    data-section-tab={item.id}
                    aria-selected={activeSection() === item.id}
                    aria-controls={panelId}
                    tabindex={activeSection() === item.id ? 0 : -1}
                    disabled={isSearching()}
                    onClick={() => setActiveSection(item.id)}
                  >
                    {item.label}
                  </button>
                ))}
              </div>

              <div
                ref={contentRef}
                id={panelId}
                class="settings-content"
                classList={{ "settings-content-search": isSearching() }}
                role="tabpanel"
                aria-labelledby={`${tabId}-${activeSection()}`}
              >
                <Show
                  when={isSearching()}
                  fallback={
                    <Switch>
                      <Match when={activeSection() === "editor"}><EditorSection /></Match>
                      <Match when={activeSection() === "files"}><FilesSection /></Match>
                      <Match when={activeSection() === "preview"}><PreviewSection /></Match>
                      <Match when={activeSection() === "ai"}><AiSection /></Match>
                      <Match when={activeSection() === "programs"}><ProgramsSection /></Match>
                      <Match when={activeSection() === "appearance"}><AppearanceSection /></Match>
                      <Match when={activeSection() === "sidebar"}><SidebarSettingsSection /></Match>
                      <Match when={activeSection() === "updates"}><UpdatesSection /></Match>
                      <Match when={activeSection() === "shortcuts"}><ShortcutsSection /></Match>
                      <Match when={activeSection() === "advanced"}><AdvancedSection /></Match>
                    </Switch>
                  }
                >
                  <Show
                    when={!noMatches()}
                    fallback={
                      <div class="settings-empty">
                        <div class="settings-empty-title">No settings match “{query()}”</div>
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
