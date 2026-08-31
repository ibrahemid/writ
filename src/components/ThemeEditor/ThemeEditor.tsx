import { createSignal, createEffect, onCleanup, For, Show } from "solid-js";
import { themeStore } from "../../stores/global/theme";
import { configStore } from "../../stores/global/config";
import { useWindow } from "../WindowProvider/WindowProvider";
import { installFocusTrap } from "../../lib/focus-trap";
import { TOKEN_GROUPS, GROUP_LABELS, tokenKey } from "../../types/theme";
import type { TokenGroup, Theme, ThemeConfig } from "../../types/theme";
import Button from "../Button/Button";
import Tooltip from "../Tooltip/Tooltip";
import { showToast } from "../Notifications/Toast";
import "./ThemeEditor.css";

// Singleton state — Writ is single-window
const [isOpen, setIsOpen] = createSignal(false);
let openSnapshot: ThemeConfig | null = null;

export function openThemeEditor() {
  openSnapshot = themeStore.toConfig();
  setIsOpen(true);
}

export function closeThemeEditor() {
  if (openSnapshot) {
    themeStore.loadConfig(openSnapshot);
  }
  openSnapshot = null;
  setIsOpen(false);
}

function tokensForGroup(theme: Theme, group: TokenGroup): Record<string, string> {
  return (theme[group] ?? {}) as Record<string, string>;
}

export default function ThemeEditor() {
  const win = useWindow();
  let modalRef: HTMLDivElement | undefined;

  function valueFor(group: TokenGroup, name: string): string {
    return themeStore.resolvedTokens()[tokenKey(group, name)];
  }

  function handleSwatchInput(group: TokenGroup, name: string, value: string) {
    themeStore.setOverride(tokenKey(group, name), value);
  }

  function handlePresetChange(id: string) {
    themeStore.setPreset(id);
  }

  function handleReset() {
    themeStore.resetOverrides();
  }

  async function handleSave() {
    try {
      await configStore.save({
        ...configStore.config(),
        theme: themeStore.toConfig(),
      });
      openSnapshot = themeStore.toConfig();
      showToast("Theme saved", "success");
    } catch {
      showToast("Failed to save theme", "error");
    }
  }

  createEffect(() => {
    if (!isOpen() || !modalRef) return;
    const teardown = installFocusTrap(modalRef, {
      onEscape: () => closeThemeEditor(),
      fallbackRestore: () => {
        win.editor.focusEditor();
        return null;
      },
    });
    onCleanup(teardown);
  });

  return (
    <Show when={isOpen()}>
      <div class="theme-editor-overlay" onClick={() => closeThemeEditor()}>
        <div
          ref={modalRef}
          class="theme-editor"
          onClick={(e) => e.stopPropagation()}
          role="dialog"
          aria-modal="true"
          aria-label="Customize theme"
        >
          <div class="theme-editor-header">
            <div class="theme-editor-title">Customize theme</div>
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
              <Button data-action="reset-theme" onClick={handleReset}>
                Reset
              </Button>
              <Button variant="primary" data-action="save-theme" onClick={handleSave}>
                Save
              </Button>
              <Tooltip label="Close theme editor">
                <Button
                  variant="ghost"
                  icon="x"
                  iconSize={16}
                  onClick={closeThemeEditor}
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
                    <For each={Object.keys(tokensForGroup(themeStore.activePreset(), group))}>
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
                            aria-label={tokenKey(group, name)}
                          />
                          <span class="theme-editor-name">{name}</span>
                          <span class="theme-editor-hex">{valueFor(group, name)}</span>
                        </label>
                      )}
                    </For>
                  </div>
                  <Show when={group === "accent"}>
                    <p class="theme-editor-note">
                      The Accent colour setting paints the accent tokens until one is set here.
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
