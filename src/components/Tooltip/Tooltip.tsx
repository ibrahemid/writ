import {
  Show,
  children as resolveChildren,
  createEffect,
  createSignal,
  createUniqueId,
  onCleanup,
  type JSX,
} from "solid-js";
import "./Tooltip.css";

interface TooltipProps {
  label: string;
  placement?: "top" | "bottom";
  children: JSX.Element;
}

/** Hover has to settle before a tip appears; focus does not. */
const HOVER_DELAY_MS = 500;
/** Space between the tip and the element it describes. */
const GAP = 6;
/** Keeps the tip off the very edge of the viewport. */
const EDGE_GAP = 4;

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(value, max));
}

export default function Tooltip(props: TooltipProps) {
  const id = createUniqueId();
  const [visible, setVisible] = createSignal(false);
  const [size, setSize] = createSignal({ width: 0, height: 0 });
  const resolved = resolveChildren(() => props.children);
  let anchorRef: HTMLSpanElement | undefined;
  let tipRef: HTMLDivElement | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;

  function cancelTimer() {
    if (timer === undefined) return;
    clearTimeout(timer);
    timer = undefined;
  }

  function show() {
    cancelTimer();
    setVisible(true);
  }

  function hide() {
    cancelTimer();
    setVisible(false);
    setSize({ width: 0, height: 0 });
  }

  function scheduleShow() {
    cancelTimer();
    timer = setTimeout(() => {
      timer = undefined;
      setVisible(true);
    }, HOVER_DELAY_MS);
  }

  onCleanup(cancelTimer);

  // Escape dismisses even when focus never entered the tip's anchor — a
  // pointer can hover a control while the caret stays in the editor.
  createEffect(() => {
    if (!visible()) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") hide();
    };
    document.addEventListener("keydown", onKeyDown, true);
    onCleanup(() => document.removeEventListener("keydown", onKeyDown, true));
  });

  // The description belongs on the control itself, not on the wrapper, so a
  // screen reader announces it with the control's own name.
  createEffect(() => {
    const target = resolved.toArray().find((node) => node instanceof Element);
    if (!(target instanceof Element)) return;
    if (visible()) target.setAttribute("aria-describedby", id);
    else target.removeAttribute("aria-describedby");
  });

  // Measured after the tip paints, so clamping uses the real box.
  createEffect(() => {
    if (!visible()) return;
    const el = tipRef;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    setSize((prev) =>
      prev.width === rect.width && prev.height === rect.height
        ? prev
        : { width: rect.width, height: rect.height },
    );
  });

  function positionStyle(): JSX.CSSProperties {
    const anchor = anchorRef?.getBoundingClientRect();
    if (!anchor) return { left: "0px", top: "0px" };
    const { width, height } = size();
    const maxLeft = Math.max(EDGE_GAP, window.innerWidth - width - EDGE_GAP);
    const maxTop = Math.max(EDGE_GAP, window.innerHeight - height - EDGE_GAP);

    const above = anchor.top - GAP - height;
    const below = anchor.bottom + GAP;
    const prefersBottom = props.placement === "bottom";
    const fits = prefersBottom ? below + height + EDGE_GAP <= window.innerHeight : above >= EDGE_GAP;
    const top = prefersBottom ? (fits ? below : above) : fits ? above : below;

    return {
      left: `${clamp(anchor.left + anchor.width / 2 - width / 2, EDGE_GAP, maxLeft)}px`,
      top: `${clamp(top, EDGE_GAP, maxTop)}px`,
    };
  }

  return (
    <span
      ref={(el) => (anchorRef = el)}
      class="writ-tooltip-anchor"
      onPointerEnter={scheduleShow}
      onPointerLeave={hide}
      onFocusIn={show}
      onFocusOut={hide}
    >
      {resolved()}
      <Show when={visible()}>
        <div
          ref={(el) => (tipRef = el)}
          id={id}
          role="tooltip"
          class="writ-tooltip"
          style={positionStyle()}
        >
          {props.label}
        </div>
      </Show>
    </span>
  );
}
