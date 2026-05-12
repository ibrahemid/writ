import { createMemo } from "solid-js";
import { useCommand } from "../../commands/registry";
import { useEffectiveBinding } from "../../commands/keybindings";
import Kbd from "../Kbd/Kbd";
import "./StatusBar.css";

export default function StatusBar() {
  const paletteBinding = createMemo(() =>
    useEffectiveBinding("palette.open", useCommand("palette.open")?.keybinding),
  );

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
