import { createSignal } from "solid-js";
import { configStore } from "../../stores/global/config";
import { requestChoice } from "../../stores/global/dialogs";
import { SECTION_ORDER, settingEntry, type SettingEntry, type SettingsSection } from "../../settings";
import { isSectionAvailable } from "../../settings/availability";
import type { AppId } from "../../types/config";
import { discardShortcutDrafts, hasUnsavedShortcuts } from "../ShortcutEditor/ShortcutEditor";
import { openDueApps } from "../../commands/app-surfaces";
import { setRecordingId, WINDOW_CHORD_RECORDING } from "./recording";

// Singleton state — Writ is single-window

export const DEFAULT_SECTION: SettingsSection = SECTION_ORDER[0];

const [isOpen, setIsOpen] = createSignal(false);
const [activeSection, setActiveSection] = createSignal<SettingsSection>(DEFAULT_SECTION);
const [activePage, setActivePage] = createSignal<AppId | null>(null);
const [query, setQuery] = createSignal("");
const [highlightId, setHighlightId] = createSignal<string | null>(null);
const [returnedFrom, setReturnedFrom] = createSignal<AppId | null>(null);

export {
  isOpen,
  activeSection,
  activePage,
  query,
  setQuery,
  highlightId,
  setHighlightId,
  returnedFrom,
};

export const isSearching = () => query().trim().length > 0;

function pageFor(entry: SettingEntry): AppId | null {
  const pages = entry.pages ?? [];
  return pages.find((app) => configStore.isAppOn(app)) ?? pages[0] ?? null;
}

export function openSettings(section?: SettingsSection, settingId?: string) {
  setQuery("");
  const entry = settingId ? settingEntry(settingId) : undefined;
  const wanted = entry?.section ?? section;
  if (wanted !== undefined) {
    // A section the platform cannot fill has no nav item, so landing on one would
    // leave no way back out of it.
    const target = isSectionAvailable(wanted) ? wanted : DEFAULT_SECTION;
    setActiveSection(target);
    setActivePage(entry && target === entry.section ? pageFor(entry) : null);
  }
  setReturnedFrom(null);
  setHighlightId(settingId ?? null);
  setIsOpen(true);
}

export function closeSettings() {
  setHighlightId(null);
  setIsOpen(false);
  discardShortcutDrafts();
  // After the dialog has handed focus back, so a surface that takes focus
  // keeps it.
  queueMicrotask(openDueApps);
}

/** Opens Settings at the window's chord, listening for a new one when
 * `record` is set: for a chord another app holds. */
export function openWindowChordSetting(record: boolean) {
  openSettings("general", "hotkey.toggle");
  if (record) setRecordingId(WINDOW_CHORD_RECORDING);
}

export async function requestCloseSettings(): Promise<boolean> {
  if (hasUnsavedShortcuts()) {
    const outcome = await requestChoice({
      title: "Discard your changes?",
      message: "Writ will put the shortcuts back the way they were.",
      confirmLabel: "Discard",
      defaultAction: "cancel",
    });
    if (outcome !== "confirm") return false;
  }
  closeSettings();
  return true;
}

export async function leaveSettings(then: () => void): Promise<void> {
  if (await requestCloseSettings()) then();
}

export function toggleSettings() {
  if (isOpen()) void requestCloseSettings();
  else openSettings();
}

export function selectSection(next: SettingsSection) {
  setActiveSection(next);
  setActivePage(null);
  setReturnedFrom(null);
}

export function openAppPage(app: AppId) {
  setQuery("");
  setActiveSection("apps");
  setReturnedFrom(null);
  setActivePage(app);
}

export function closeAppPage() {
  setReturnedFrom(activePage());
  setActivePage(null);
}
