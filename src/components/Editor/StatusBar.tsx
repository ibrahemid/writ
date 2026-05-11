import { createMemo } from "solid-js";
import { getCommand } from "../../commands/registry";
import Kbd from "../Kbd/Kbd";
import "./StatusBar.css";

export default function StatusBar() {
  const paletteBinding = createMemo(() => getCommand("palette.open")?.keybinding);

  return (
    <div class="statusbar">
      <div class="statusbar-left">
        <span class="statusbar-dot" aria-hidden="true" />
        <span class="statusbar-label">autosaved</span>
      </div>
      <div class="statusbar-spacer" />
      <div class="statusbar-right">
        <Kbd binding={paletteBinding()} />
        <span class="statusbar-label">command palette</span>
      </div>
    </div>
  );
}
