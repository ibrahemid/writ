import { createEffect, onCleanup, For, Show, Switch, Match } from "solid-js";
import { installFocusTrap } from "../../lib/focus-trap";
import { useWindow } from "../WindowProvider/WindowProvider";
import {
  SECTION_LABELS,
  SECTION_ORDER,
  matchedSettingIds,
  rankSections,
  rankSettings,
  type SettingsSection,
} from "../../settings";
import { isSectionAvailable, isSettingAvailable } from "../../settings/availability";
import Button from "../Button/Button";
import Icon from "../Icon/Icon";
import Tooltip from "../Tooltip/Tooltip";
import { openSearchPalette } from "../SearchPalette/SearchPalette";
import { SearchContext, type SearchContextValue } from "./SettingsRow";
import { isRecordingChord } from "./recording";
import {
  activeSection,
  closeSettings,
  highlightId,
  isOpen,
  isSearching,
  leaveSettings,
  openSettings,
  query,
  requestCloseSettings,
  selectSection,
  setHighlightId,
  setQuery,
  toggleSettings,
} from "./state";
import GeneralSection from "./sections/General";
import EditorSection from "./sections/Editor";
import AppearanceSection from "./sections/Appearance";
import AppsSection from "./sections/Apps";
import IntegrationsSection from "./sections/Integrations";
import ShortcutsSection from "./sections/Shortcuts";
import AdvancedSection from "./sections/Advanced";
import AboutSection from "./sections/About";
import "./SettingsModal.css";

export { openSettings, closeSettings, toggleSettings };

const NAV_ITEMS: { id: SettingsSection; label: string }[] = SECTION_ORDER.filter(
  isSectionAvailable,
).map((id) => ({ id, label: SECTION_LABELS[id] }));

function AllSections() {
  return (
    <>
      <GeneralSection />
      <EditorSection />
      <AppearanceSection />
      <AppsSection />
      <IntegrationsSection />
      <ShortcutsSection />
      <AdvancedSection />
      <AboutSection />
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

  const sectionResults = () => rankSections(query()).filter(isSectionAvailable);

  const searchContext: SearchContextValue = {
    rowVisible: (id) => !isSearching() || (matched().has(id) && isSettingAvailable(id)),
    sectionVisible: (section) => !isSearching() || matchedSections().has(section),
    highlighted: (id) => highlightId() === id,
  };

  // Focus follows the selection: roving tabindex has just set every other tab
  // to -1, so a tab that keeps focus without keeping the selection is one Tab
  // can no longer come back to.
  function focusSection(next: SettingsSection) {
    selectSection(next);
    requestAnimationFrame(() =>
      navRef?.querySelector<HTMLButtonElement>(`[data-section-tab="${next}"]`)?.focus(),
    );
  }

  function moveSection(delta: number) {
    const ids = NAV_ITEMS.map((item) => item.id);
    const current = ids.indexOf(activeSection());
    focusSection(ids[(current + delta + ids.length) % ids.length]);
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
        focusSection(NAV_ITEMS[0].id);
        break;
      case "End":
        event.preventDefault();
        focusSection(NAV_ITEMS[NAV_ITEMS.length - 1].id);
        break;
    }
  }

  function openSectionResult(section: SettingsSection) {
    setQuery("");
    focusSection(section);
  }

  const noMatches = () =>
    isSearching() &&
    sectionResults().length === 0 &&
    rankSettings(query()).every((entry) => !isSettingAvailable(entry.id));

  createEffect(() => {
    if (!isOpen() || !modalRef) return;
    const teardown = installFocusTrap(modalRef, {
      // While a recorder waits for a chord, Escape cancels the recording.
      isActive: () => !isRecordingChord(),
      onEscape: () => void requestCloseSettings(),
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
        <div class="settings-overlay" onClick={() => void requestCloseSettings()}>
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
              <Tooltip label="Close settings" chord="Escape">
                <Button
                  variant="ghost"
                  icon="x"
                  iconSize={16}
                  onClick={() => void requestCloseSettings()}
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
                    onClick={() => selectSection(item.id)}
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
                      <Match when={activeSection() === "general"}><GeneralSection /></Match>
                      <Match when={activeSection() === "editor"}><EditorSection /></Match>
                      <Match when={activeSection() === "appearance"}><AppearanceSection /></Match>
                      <Match when={activeSection() === "apps"}><AppsSection /></Match>
                      <Match when={activeSection() === "integrations"}><IntegrationsSection /></Match>
                      <Match when={activeSection() === "shortcuts"}><ShortcutsSection /></Match>
                      <Match when={activeSection() === "advanced"}><AdvancedSection /></Match>
                      <Match when={activeSection() === "about"}><AboutSection /></Match>
                    </Switch>
                  }
                >
                  <Show
                    when={!noMatches()}
                    fallback={
                      <div class="settings-empty">
                        <div class="settings-empty-title">No settings match “{query()}”</div>
                        <Button
                          data-action="search-everywhere"
                          onClick={() => {
                            const words = query().trim();
                            void leaveSettings(() => openSearchPalette(words));
                          }}
                        >
                          Search everywhere for “{query().trim()}”
                        </Button>
                      </div>
                    }
                  >
                    <Show when={sectionResults().length > 0}>
                      <div class="settings-section-results" data-section-results>
                        <For each={sectionResults()}>
                          {(section) => (
                            <button
                              type="button"
                              class="settings-section-result"
                              data-section-result={section}
                              onClick={() => openSectionResult(section)}
                            >
                              <span>{SECTION_LABELS[section]}</span>
                              <Icon name="caret-right" size={14} />
                            </button>
                          )}
                        </For>
                      </div>
                    </Show>
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
