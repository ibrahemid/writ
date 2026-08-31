import { For, Show, createEffect, createMemo, createSignal } from "solid-js";
import Button from "../Button/Button";
import Tooltip from "../Tooltip/Tooltip";
import SearchBar from "../Sidebar/SearchBar";
import type { IconName } from "../Icon/Icon";
import type { ActiveFormats } from "../../types/editor";
import { useWindow } from "../WindowProvider/WindowProvider";
import { executeCommand, useCommand } from "../../commands/registry";
import { useEffectiveBinding } from "../../commands/keybindings";
import { formatKeybinding } from "../../lib/keybinding-format";
import { resolvePlatform } from "../../lib/platform";
import { resolveChromeLayout, resolveLightsSlot } from "../../lib/window-chrome";
import { osWindowStore } from "../../stores/global/os-window";
import TrafficLights from "../TitleBar/TrafficLights";
import "./Toolbar.css";

interface FormatControl {
  command: string;
  icon: IconName;
  label: string;
  /** The chord the command carries when it is registered. */
  chord?: string;
  /**
   * The construct the control toggles. A control without one — Link, which
   * inserts rather than toggles — carries no pressed state.
   */
  flag?: keyof ActiveFormats;
}

// Formatting applies to prose, so each control is live only while the editor
// holds a markdown buffer — which is exactly when its command is registered.
const FORMAT_CONTROLS: readonly FormatControl[] = [
  { command: "editor.toggleBold", icon: "text-b", label: "Bold", chord: "CmdOrCtrl+B", flag: "bold" },
  { command: "editor.toggleItalic", icon: "text-italic", label: "Italic", chord: "CmdOrCtrl+I", flag: "italic" },
  { command: "editor.toggleInlineCode", icon: "code", label: "Code", chord: "CmdOrCtrl+Shift+E", flag: "code" },
  { command: "editor.insertLink", icon: "link-simple", label: "Link", chord: "CmdOrCtrl+K" },
  { command: "editor.toggleBulletList", icon: "list-bullets", label: "Bulleted list", flag: "bullet" },
  { command: "editor.toggleTaskList", icon: "list-checks", label: "Task list", flag: "task" },
];

function tip(label: string, binding: string | undefined): string {
  const chord = formatKeybinding(binding);
  return chord ? `${label} ${chord}` : label;
}

export default function Toolbar() {
  const win = useWindow();
  // Read per mount: the platform layer is written once at boot (ADR-030), so a
  // reactive read would only cost a navigator lookup per render.
  const platform = resolvePlatform();
  const layout = resolveChromeLayout(platform);
  // A closed sidebar takes its head with it, so the lights fall back to the
  // toolbar's leading edge rather than leaving the window undismissable.
  const lightsInToolbar = () => resolveLightsSlot(platform, win.sidebar.isOpen()) === "toolbar-lead";
  const [focusIndex, setFocusIndex] = createSignal(0);
  let barRef: HTMLDivElement | undefined;

  const available = createMemo(() =>
    FORMAT_CONTROLS.map((control) => useCommand(control.command) !== undefined).join(","),
  );

  /**
   * The roving stops: the search field keeps its own tab stop and its arrows,
   * and the window lights are chrome rather than note actions, so neither joins
   * the bar's single tab stop.
   */
  function stops(): HTMLButtonElement[] {
    if (!barRef) return [];
    return Array.from(
      barRef.querySelectorAll<HTMLButtonElement>("button:not([disabled]):not(.maclight)"),
    );
  }

  // One tab stop for the bar. Re-runs when a formatting control goes live or
  // dead, because a disabled control is not a stop.
  createEffect(() => {
    available();
    const items = stops();
    if (items.length === 0) return;
    const active = Math.min(focusIndex(), items.length - 1);
    items.forEach((el, index) => {
      el.tabIndex = index === active ? 0 : -1;
    });
  });

  function handleKeyDown(event: KeyboardEvent) {
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (step === 0) return;
    if (event.target instanceof HTMLInputElement) return;
    const items = stops();
    if (items.length === 0) return;
    event.preventDefault();
    const current = items.findIndex((el) => el === document.activeElement);
    const next = (Math.max(current, 0) + step + items.length) % items.length;
    setFocusIndex(next);
    items[next].focus();
  }

  return (
    <div
      ref={(el) => (barRef = el)}
      class="writ-toolbar"
      role="toolbar"
      aria-label="Note actions"
      data-platform={platform}
      // The bar is the drag region on macOS, where the window has no title bar
      // of its own. The attribute stays off the controls so a press on a button
      // clicks it rather than moving the window.
      data-tauri-drag-region={platform === "mac" ? "" : undefined}
      onKeyDown={handleKeyDown}
    >
      <Show when={lightsInToolbar()}>
        <TrafficLights focused={osWindowStore.focused()} />
      </Show>

      <Tooltip label={tip("Toggle sidebar", useEffectiveBinding("sidebar.toggle", "CmdOrCtrl+\\"))}>
        <Button
          variant="ghost"
          class="writ-toolbar-btn"
          icon="sidebar-simple"
          aria-label="Toggle sidebar"
          onClick={() => executeCommand("sidebar.toggle")}
        />
      </Tooltip>

      {/* GNOME merges compose into the header bar with the window title. */}
      <Show when={!layout.composeInChrome}>
        <Tooltip label={tip("New note", useEffectiveBinding("note.new", "CmdOrCtrl+N"))}>
          <Button
            variant="ghost"
            class="writ-toolbar-compose"
            icon="note-pencil"
            onClick={() => executeCommand("note.new")}
          >
            New note
          </Button>
        </Tooltip>

        <div class="writ-toolbar-divider" role="separator" aria-orientation="vertical" />
      </Show>

      <div class="writ-toolbar-cluster">
        <For each={FORMAT_CONTROLS}>
          {(control) => (
            <Tooltip label={tip(control.label, useEffectiveBinding(control.command, control.chord))}>
              <Button
                variant="ghost"
                class="writ-toolbar-format"
                icon={control.icon}
                aria-label={control.label}
                pressed={control.flag ? win.editor.activeFormats()[control.flag] : undefined}
                disabled={useCommand(control.command) === undefined}
                onClick={() => executeCommand(control.command)}
              />
            </Tooltip>
          )}
        </For>
      </div>

      {/* GNOME keeps search in the sidebar's own header segment. */}
      <Show when={!layout.headerBar}>
        <SearchBar />
      </Show>
    </div>
  );
}
