import { Show, createSignal, createEffect, onMount, onCleanup, on } from "solid-js";
import { linkStore } from "../../stores/global/link";
import type { LinkVerdict } from "../../types/link";
import Button from "../Button/Button";
import Tooltip from "../Tooltip/Tooltip";
import "./LinkConfirm.css";

export interface LinkRequest {
  id: number;
  href: string;
  // Click coordinates inside the preview iframe's viewport.
  x: number;
  y: number;
}

interface Props {
  request: LinkRequest;
  // Rect of the iframe the click came from, so the popover can be placed in
  // pane coordinates and clamped to it.
  frameRect: () => { width: number; height: number };
  onDismiss: () => void;
}

const POPOVER_WIDTH = 288;
const POPOVER_HEIGHT = 108;
const EDGE_GAP = 8;
const CURSOR_GAP = 12;

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

// The host is the only part of an address that says where the click actually
// goes, so it is what the popover leads with; the full URL sits under it as
// the detail.
//
// Only ever called with the classified URL Rust returned, never with the raw
// href from the frame. Reading a host out of an unvetted string would be a
// second URL parser making a claim the one policy never approved, and a
// credentialed `https://user:pw@evil.example/` would render a clean-looking
// host for a link that is refused.
export function hostLabel(url: string): string {
  try {
    const parsed = new URL(url);
    if (parsed.host) return parsed.host;
    if (parsed.protocol === "mailto:") return decodeURIComponent(parsed.pathname);
  } catch {
    // Unreachable for a classified URL; fall through to the whole string.
  }
  return url;
}

export default function LinkConfirm(props: Props) {
  const [verdict, setVerdict] = createSignal<LinkVerdict | null>(null);
  // The popover grows a row when a link is refused, so the bottom-edge clamp
  // reads the rendered height rather than trusting the constant.
  const [measuredHeight, setMeasuredHeight] = createSignal(0);
  let popoverRef: HTMLDivElement | undefined;
  let openRef: HTMLButtonElement | undefined;
  let cancelRef: HTMLButtonElement | undefined;
  const restoreTo = document.activeElement;

  // Re-classified for every request, so a burst that replaces the popover
  // never leaves the previous link's verdict on screen.
  createEffect(
    on(
      () => props.request.id,
      () => {
        const href = props.request.href;
        setVerdict(null);
        void linkStore
          .classify(href)
          .then((result) => {
            if (props.request.href === href) setVerdict(result);
          })
          .catch(() => {
            if (props.request.href === href) {
              setVerdict({
                allowed: false,
                url: null,
                reason: "unparseable",
                message: "That link is not a valid web address.",
              });
            }
          });
      },
    ),
  );

  // The normalized destination once the policy has one, the raw href until
  // then and for a refusal, where there is nothing else to show.
  function destination(): string {
    return verdict()?.url ?? props.request.href;
  }

  function position(): { left: number; top: number } {
    const rect = props.frameRect();
    const height = measuredHeight() || POPOVER_HEIGHT;
    const left = clamp(
      props.request.x + CURSOR_GAP,
      EDGE_GAP,
      Math.max(EDGE_GAP, rect.width - POPOVER_WIDTH - EDGE_GAP),
    );
    const top = clamp(
      props.request.y + CURSOR_GAP,
      EDGE_GAP,
      Math.max(EDGE_GAP, rect.height - height - EDGE_GAP),
    );
    return { left, top };
  }

  function open() {
    void linkStore.openExternal(props.request.href);
    props.onDismiss();
  }

  function onKeyDown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.preventDefault();
    props.onDismiss();
  }

  function onPointerDown(event: PointerEvent) {
    const target = event.target;
    if (target instanceof Node && popoverRef?.contains(target)) return;
    props.onDismiss();
  }

  onMount(() => {
    document.addEventListener("keydown", onKeyDown);
    // Captured, so a pointerdown a control would otherwise swallow still
    // dismisses. A click inside the iframe never reaches the shell, so a click
    // on the preview itself leaves the popover up; the next link click replaces
    // it. Dismissing on window blur would instead close the popover on an
    // app switch, which is the moment someone checks a domain before opening it.
    document.addEventListener("pointerdown", onPointerDown, true);
    queueMicrotask(() => (openRef ?? popoverRef)?.focus());
  });

  createEffect(() => {
    // Tracks the verdict so the refusal row is included once it renders.
    verdict();
    const height = popoverRef?.offsetHeight ?? 0;
    if (height > 0) setMeasuredHeight(height);
  });

  // A refusal removes the Open button; keep focus in the popover rather than
  // letting it fall back to the document.
  createEffect(
    on(
      () => verdict()?.allowed === false,
      (refused) => {
        if (refused && popoverRef?.contains(document.activeElement)) cancelRef?.focus();
      },
      { defer: true },
    ),
  );

  onCleanup(() => {
    document.removeEventListener("keydown", onKeyDown);
    document.removeEventListener("pointerdown", onPointerDown, true);
    if (restoreTo instanceof HTMLElement && restoreTo.isConnected) restoreTo.focus();
  });

  return (
    <div
      ref={popoverRef}
      class="link-confirm"
      role="dialog"
      aria-label="Link destination"
      tabIndex={-1}
      style={{ left: `${position().left}px`, top: `${position().top}px` }}
    >
      {/* The host line makes a claim about where the click goes, so it exists
          only once the policy has approved a destination to make it about. */}
      <Show when={verdict()?.url}>
        {(url) => <p class="link-confirm-host">{hostLabel(url())}</p>}
      </Show>
      <Tooltip label={destination()}>
        <p class="link-confirm-url">{destination()}</p>
      </Tooltip>
      <Show when={verdict()?.allowed === false}>
        <p class="link-confirm-refused" role="status">
          {verdict()?.message}
        </p>
      </Show>
      <div class="link-confirm-actions">
        <Button ref={cancelRef} variant="ghost" class="link-confirm-cancel" onClick={props.onDismiss}>
          Cancel
        </Button>
        {/* Present but inert until the verdict lands, so focus has a stable
            home and a refused link never offers an action that would fail. */}
        <Show when={verdict()?.allowed !== false}>
          <Button
            ref={openRef}
            variant="primary"
            class="link-confirm-open"
            disabled={verdict() === null}
            onClick={open}
          >
            Open
          </Button>
        </Show>
      </div>
    </div>
  );
}
