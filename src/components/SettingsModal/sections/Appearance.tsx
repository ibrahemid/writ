import { For } from "solid-js";
import {
  configStore,
  INTERFACE_TEXT_MIN,
  INTERFACE_TEXT_MAX,
  clampInterfaceTextSize,
} from "../../../stores/global/config";
import { themeStore } from "../../../stores/global/theme";
import { PRESETS } from "../../../styles/themes";
import { ACCENTS, TYPE } from "../../../styles/generated/tokens";
import { resolvePlatform } from "../../../lib/platform";
import { settingTitle } from "../../../settings";
import type { AccentId, AppearanceConfig, Polarity, ProseFaceId } from "../../../types/config";
import Button from "../../Button/Button";
import { openThemeEditor } from "../../ThemeEditor/ThemeEditor";
import { SectionLabel, Segmented, parseIntSafe, patchConfig } from "../controls";
import { leaveSettings, openSettings } from "../state";
import { ProseSample, ThemeSwatches } from "../previews";
import { SettingsRow } from "../SettingsRow";

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

export default function AppearanceSection() {
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
      <SettingsRow id="appearance.polarity">
        <Segmented
          setting="appearance_polarity"
          label={settingTitle("appearance.polarity")}
          options={POLARITY_OPTIONS}
          value={appearance().polarity}
          onChange={(polarity) => patchAppearance({ polarity })}
        />
      </SettingsRow>
      <SettingsRow
        id="appearance.accent"
        caution={themeStore.accentApplies() ? undefined : "The current theme sets its own accent."}
      >
        <div
          ref={accentsRef}
          class="settings-accents"
          role="radiogroup"
          aria-label={settingTitle("appearance.accent")}
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
        labelFor="setting-appearance-prose-face"
        detail={<ProseSample />}
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
      <SettingsRow
        id="appearance.theme"
        labelFor="setting-theme-preset"
        detail={<ThemeSwatches />}
      >
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
      <SettingsRow id="appearance.custom_colors">
        <Button
          data-action="open-theme-editor"
          onClick={() => {
            void leaveSettings(() =>
              openThemeEditor(() => openSettings("appearance", "appearance.custom_colors")),
            );
          }}
        >
          Customize theme…
        </Button>
      </SettingsRow>
    </div>
  );
}
