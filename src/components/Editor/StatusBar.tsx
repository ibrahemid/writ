import { createMemo } from "solid-js";
import { getCommand } from "../../commands/registry";
import { formatKeybinding } from "../../lib/keybinding-format";
import "./StatusBar.css";

export default function StatusBar() {
  const paletteHint = createMemo(() => formatKeybinding(getCommand("palette.open")?.keybinding));

  return (
    <div class="statusbar">
      <div class="statusbar-left">
        <span class="statusbar-dot" aria-hidden="true" />
        <span class="statusbar-label">autosaved</span>
      </div>
      <div class="statusbar-spacer" />
      <div class="statusbar-right">
        <span class="statusbar-kbd">{paletteHint()}</span>
        <span class="statusbar-label">command palette</span>
      </div>
    </div>
  );
}
