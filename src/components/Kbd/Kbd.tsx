import { For, Show } from "solid-js";
import { keybindingSegments, spokenKeybinding } from "../../lib/keybinding-format";
import "./Kbd.css";

interface KbdProps {
  binding?: string;
  muted?: boolean;
}

export default function Kbd(props: KbdProps) {
  const segments = () => keybindingSegments(props.binding);

  return (
    <span
      class={`kbd-chord ${props.muted ? "kbd-chord-muted" : ""}`}
      role="img"
      aria-label={spokenKeybinding(props.binding) || "No shortcut"}
    >
      <Show
        when={segments().length > 0}
        fallback={<span class="kbd-key kbd-key-empty">—</span>}
      >
        <For each={segments()}>
          {(segment) => <span class="kbd-key">{segment}</span>}
        </For>
      </Show>
    </span>
  );
}
