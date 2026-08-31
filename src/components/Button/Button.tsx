import { Show, createEffect, type JSX } from "solid-js";
import Icon, { type IconName } from "../Icon/Icon";
import { logFailure } from "../../lib/log";
import "./Button.css";

interface ButtonProps {
  /**
   * `ref={variable}` and `ref={fn}` both reach the button. Solid compiles the
   * variable form into a setter, so what arrives here is always callable.
   */
  ref?: HTMLButtonElement | ((el: HTMLButtonElement) => void);
  variant?: "primary" | "secondary" | "ghost";
  icon?: IconName;
  iconSize?: number;
  danger?: boolean;
  disabled?: boolean;
  /** Set only on a toggle: renders aria-pressed and the on state. */
  pressed?: boolean;
  type?: "button" | "submit";
  onClick?: (event: MouseEvent) => void;
  "aria-label"?: string;
  /** Set while the click's work is still running. */
  "aria-busy"?: boolean;
  /** Test and automation handle for a button that carries no stable label. */
  "data-action"?: string;
  class?: string;
  children?: JSX.Element;
}

export default function Button(props: ButtonProps) {
  const variant = () => props.variant ?? "secondary";
  const classes = () =>
    ["writ-btn", `writ-btn-${variant()}`, props.danger ? "writ-btn-danger" : "", props.class ?? ""]
      .filter(Boolean)
      .join(" ");

  createEffect(() => {
    if (props.children === undefined && !props["aria-label"]) {
      logFailure("an icon-only button was rendered without a name");
    }
  });

  return (
    <button
      ref={props.ref as HTMLButtonElement | undefined}
      type={props.type ?? "button"}
      class={classes()}
      disabled={props.disabled}
      aria-label={props["aria-label"]}
      aria-busy={props["aria-busy"]}
      aria-pressed={props.pressed}
      data-action={props["data-action"]}
      onClick={(event) => props.onClick?.(event)}
    >
      <Show when={props.icon}>
        {(name) => <Icon name={name()} size={props.iconSize} />}
      </Show>
      {props.children}
    </button>
  );
}
