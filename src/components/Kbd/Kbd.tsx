import { For, Show } from "solid-js";
import { keybindingSegments } from "../../lib/keybinding-format";
import "./Kbd.css";

interface KbdProps {
  binding?: string;
  muted?: boolean;
}

export default function Kbd(props: KbdProps) {
  const segments = () => keybindingSegments(props.binding);

  return (
    <span class={`kbd-chord ${props.muted ? "kbd-chord-muted" : ""}`} aria-label={props.binding ?? "no shortcut"}>
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
