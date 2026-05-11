import { formatKeybinding } from "../../lib/keybinding-format";
import "./Kbd.css";

interface KbdProps {
  binding?: string;
  muted?: boolean;
}

export default function Kbd(props: KbdProps) {
  const display = () => {
    const formatted = formatKeybinding(props.binding);
    return formatted || "—";
  };

  return (
    <span class={`kbd ${props.muted ? "kbd-muted" : ""}`} aria-label={props.binding ?? "no shortcut"}>
      {display()}
    </span>
  );
}
