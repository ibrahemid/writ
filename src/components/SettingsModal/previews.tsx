import { For, Show } from "solid-js";
import { getCommand } from "../../commands/registry";
import { useEffectiveBinding } from "../../commands/effective-bindings";
import { MENU_COMMANDS } from "../../commands/menu-commands";
import { MARKDOWN_FORMAT_KEYS } from "../../editor/editor-command-keys";
import { configStore } from "../../stores/global/config";
import { themeStore } from "../../stores/global/theme";
import { ACCENTS } from "../../styles/generated/tokens";
import { getDefaultPreset, getPreset, takesAccentSetting } from "../../styles/themes";
import type { FileExtension } from "../../types/config";
import Kbd from "../Kbd/Kbd";

export interface ChordHint {
  commandId: string;
  label: string;
}

function shippedChord(commandId: string): string | undefined {
  return (
    getCommand(commandId)?.keybinding ??
    MARKDOWN_FORMAT_KEYS.find((key) => key.id === commandId)?.keybinding ??
    MENU_COMMANDS.find((entry) => entry.id === commandId)?.accelerator
  );
}

function ChordHintItem(props: { hint: ChordHint }) {
  const binding = () => useEffectiveBinding(props.hint.commandId, shippedChord(props.hint.commandId));
  return (
    <Show when={binding()}>
      {(chord) => (
        <span class="settings-chord-hint" data-chord-hint={props.hint.commandId}>
          <Kbd binding={chord()} /> {props.hint.label}
        </span>
      )}
    </Show>
  );
}

// A chord the person cleared is left out rather than shown as its old default.
export function ChordHints(props: { hints: readonly ChordHint[] }) {
  return (
    <span class="settings-chord-hints">
      <For each={props.hints}>{(hint) => <ChordHintItem hint={hint} />}</For>
    </span>
  );
}

export const FONT_SIZE_CHORDS: readonly ChordHint[] = [
  { commandId: "editor.zoomIn", label: "larger" },
  { commandId: "editor.zoomOut", label: "smaller" },
  { commandId: "editor.zoomReset", label: "back to the default" },
];

export const MARKDOWN_CHORDS: readonly ChordHint[] = [
  { commandId: "editor.toggleBold", label: "bold" },
  { commandId: "editor.toggleItalic", label: "italic" },
  { commandId: "editor.toggleStrikethrough", label: "strikethrough" },
  { commandId: "editor.toggleInlineCode", label: "code" },
  { commandId: "editor.insertLink", label: "link" },
];

export function ProseSample() {
  return (
    <span
      class="settings-prose-sample"
      data-prose-sample
      data-face={configStore.config().appearance.prose_face}
      style={{ "font-size": `${configStore.config().editor.font_size}px` }}
      aria-hidden="true"
    >
      The quick brown fox jumps over the lazy dog.
    </span>
  );
}

export function ThemeSwatches() {
  const theme = () => getPreset(themeStore.activePreset().id) ?? getDefaultPreset();
  const accent = () =>
    takesAccentSetting(theme().id)
      ? ACCENTS[configStore.config().appearance.accent][theme().polarity].base
      : theme().accent.default;
  return (
    <span
      class="settings-theme-sample"
      data-theme-sample={theme().id}
      style={{ background: theme().bg.canvas, color: theme().fg.default }}
      aria-hidden="true"
    >
      <span class="settings-theme-sample-sidebar" style={{ background: theme().bg.sidebar }} />
      <span class="settings-theme-sample-text">Aa</span>
      <span class="settings-theme-sample-accent" data-accent-swatch style={{ background: accent() }} />
    </span>
  );
}

export function FormatSample(props: { extension: FileExtension }) {
  return (
    <span class="settings-format-sample" data-format-sample={props.extension} aria-hidden="true">
      <span class="settings-format-sample-name">.{props.extension}</span>
      <Show
        when={props.extension === "md"}
        fallback={<span class="settings-format-sample-line">**Milk** and eggs</span>}
      >
        <span class="settings-format-sample-line">
          <strong>Milk</strong> and eggs
        </span>
      </Show>
    </span>
  );
}
