import { createSignal, createEffect, createUniqueId, onCleanup, For, Show } from "solid-js";
import { themeStore } from "../../stores/global/theme";
import { configStore } from "../../stores/global/config";
import { useWindow } from "../WindowProvider/WindowProvider";
import { installFocusTrap } from "../../lib/focus-trap";
import { requestChoice, requestConfirm } from "../ConfirmDialog/ConfirmDialog";
import {
  GROUP_LABELS,
  NON_EDITABLE_TOKENS,
  TOKEN_GROUPS,
  TOKEN_LABELS,
  tokenKey,
} from "../../types/theme";
import type { TokenGroup, Theme, ThemeConfig } from "../../types/theme";
import type { AppearanceConfig } from "../../types/config";
import Button from "../Button/Button";
import Tooltip from "../Tooltip/Tooltip";
import { showToast } from "../Notifications/Toast";
import "./ThemeEditor.css";

// Singleton state — Writ is single-window
const [isOpen, setIsOpen] = createSignal(false);
// The appearance rides along: picking a preset pins its polarity, so closing
// without saving has to put the polarity back as well as the preset.
let openSnapshot: { theme: ThemeConfig; appearance: AppearanceConfig } | null = null;

function snapshot(): { theme: ThemeConfig; appearance: AppearanceConfig } {
  return { theme: themeStore.toConfig(), appearance: themeStore.appearance() };
}

export function openThemeEditor() {
  openSnapshot = snapshot();
  setIsOpen(true);
}

export function closeThemeEditor() {
  if (openSnapshot) {
    themeStore.loadConfig(openSnapshot.theme, openSnapshot.appearance);
  }
  openSnapshot = null;
  setIsOpen(false);
}

function tokensForGroup(theme: Theme, group: TokenGroup): Record<string, string> {
  return (theme[group] ?? {}) as Record<string, string>;
}

export default function ThemeEditor() {
  const win = useWindow();
  const titleId = createUniqueId();
  let modalRef: HTMLDivElement | undefined;

  function isDirty(): boolean {
    if (!openSnapshot) return false;
    return JSON.stringify(snapshot()) !== JSON.stringify(openSnapshot);
  }

  async function requestClose() {
    if (!isDirty()) {
      closeThemeEditor();
      return;
    }
    const outcome = await requestChoice({
      title: "Discard your changes?",
      message: "Writ will put the colours back the way they were.",
      confirmLabel: "Discard",
      defaultAction: "cancel",
    });
    if (outcome === "confirm") closeThemeEditor();
  }

  function valueFor(group: TokenGroup, name: string): string {
    return themeStore.resolvedTokens()[tokenKey(group, name)];
  }

  function handleSwatchInput(group: TokenGroup, name: string, value: string) {
    themeStore.setOverride(tokenKey(group, name), value);
  }

  function handlePresetChange(id: string) {
    themeStore.setPreset(id);
  }

  async function handleResetAll() {
    if (Object.keys(themeStore.overrides()).length === 0) {
      themeStore.resetOverrides();
      return;
    }
    const confirmed = await requestConfirm({
      title: "Reset every colour?",
      message: "The colours you picked go back to the preset's own.",
      confirmLabel: "Reset all",
      defaultAction: "cancel",
    });
    if (confirmed) themeStore.resetOverrides();
  }

  async function handleSave() {
    try {
      await configStore.save({
        ...configStore.config(),
        theme: themeStore.toConfig(),
        appearance: themeStore.appearance(),
      });
      openSnapshot = snapshot();
      showToast("Theme saved", "success");
    } catch {
      showToast("Could not save the theme", "error");
    }
  }

  createEffect(() => {
    if (!isOpen() || !modalRef) return;
    const teardown = installFocusTrap(modalRef, {
      onEscape: () => void requestClose(),
      fallbackRestore: () => {
        win.editor.focusEditor();
        return null;
      },
    });
    // The trap above lands initial focus on the first focusable descendant
    // (the preset select), which draws a ring on open. Hand focus to the
    // container instead, in the same tick so nothing paints in between.
    // Tab still reaches the select first, with its ring, from here.
    modalRef.focus();
    onCleanup(teardown);
  });

  return (
    <Show when={isOpen()}>
      <div class="theme-editor-overlay" onClick={() => void requestClose()}>
        <div
          ref={modalRef}
          class="theme-editor"
          tabIndex={-1}
          onClick={(e) => e.stopPropagation()}
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
        >
          <div class="theme-editor-header">
            <div id={titleId} class="theme-editor-title">
              Customize theme
            </div>
            <div class="theme-editor-actions">
              <select
                class="theme-editor-preset"
                value={themeStore.activePreset().id}
                onChange={(e) => handlePresetChange(e.currentTarget.value)}
                aria-label="Preset"
              >
                <For each={themeStore.presets()}>
                  {(preset) => <option value={preset.id}>{preset.name}</option>}
                </For>
              </select>
              <Button data-action="reset-theme" onClick={() => void handleResetAll()}>
                Reset all
              </Button>
              <Button variant="primary" data-action="save-theme" onClick={handleSave}>
                Save
              </Button>
              <Tooltip label="Close theme editor">
                <Button
                  variant="ghost"
                  icon="x"
                  iconSize={16}
                  onClick={() => void requestClose()}
                  aria-label="Close theme editor"
                />
              </Tooltip>
            </div>
          </div>

          <div class="theme-editor-body">
            <For each={TOKEN_GROUPS}>
              {(group) => (
                <section class="theme-editor-group">
                  <h3 class="theme-editor-group-title">{GROUP_LABELS[group]}</h3>
                  <div class="theme-editor-tokens">
                    <For
                      each={Object.keys(tokensForGroup(themeStore.activePreset(), group)).filter(
                        (leaf) => !NON_EDITABLE_TOKENS.has(tokenKey(group, leaf)),
                      )}
                    >
                      {(name) => (
                        <label class="theme-editor-row">
                          <span
                            class="theme-editor-swatch"
                            style={{ background: valueFor(group, name) }}
                            aria-hidden="true"
                          />
                          <input
                            type="color"
                            class="theme-editor-picker"
                            value={valueFor(group, name)}
                            onInput={(e) => handleSwatchInput(group, name, e.currentTarget.value)}
                            data-token={tokenKey(group, name)}
                            aria-label={TOKEN_LABELS[tokenKey(group, name)]}
                          />
                          <span class="theme-editor-name">
                            {TOKEN_LABELS[tokenKey(group, name)]}
                          </span>
                          <span class="theme-editor-hex">{valueFor(group, name)}</span>
                        </label>
                      )}
                    </For>
                  </div>
                  <Show when={group === "accent" && themeStore.accentApplies()}>
                    <p class="theme-editor-note">
                      The Accent color setting paints the accent tokens until one is set here.
                    </p>
                  </Show>
                </section>
              )}
            </For>
          </div>
        </div>
      </div>
    </Show>
  );
}
