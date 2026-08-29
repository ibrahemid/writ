import { Show } from "solid-js";
import type { IconName } from "./sprite.generated";
import "./Icon.css";

interface IconProps {
  name: IconName;
  /** Overrides the platform default (--writ-icon-size). */
  size?: number;
  /** Accessible name; omit for a decorative icon (the default). */
  label?: string;
  class?: string;
}

export default function Icon(props: IconProps) {
  return (
    <svg
      class={`writ-icon ${props.class ?? ""}`.trim()}
      style={
        props.size === undefined
          ? undefined
          : { width: `${props.size}px`, height: `${props.size}px` }
      }
      viewBox="0 0 256 256"
      role={props.label ? "img" : undefined}
      aria-hidden={props.label ? undefined : "true"}
      aria-label={props.label}
    >
      <Show when={props.label}>{(label) => <title>{label()}</title>}</Show>
      <use href={`#ph-${props.name}`} />
    </svg>
  );
}

export type { IconName };
