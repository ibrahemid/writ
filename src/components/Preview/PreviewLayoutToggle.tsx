import { For, Show, createMemo } from "solid-js";
import { rendererRegistry } from "../../stores/global/renderer-registry";
import { useWindow } from "../WindowProvider/WindowProvider";
import { useActiveBuffer } from "../../lib/use-active-buffer";
import { contentTypeForBuffer } from "../../lib/content-type";
import { defaultSplit, type LayoutKind, type LayoutMode } from "../../lib/preview-layout";
import "./preview-layout-toggle.css";

// A compact three-way segmented control for the active buffer's preview
// layout: source-only, split, or preview-only. Lives in the StatusBar so it
// is reachable from every layout mode (the in-pane chip vanishes in
// source-only). Shown only when the active buffer has a registered renderer.
//
// Single-select → WAI-ARIA radiogroup: role=radio + aria-checked, roving
// tabindex (only the active segment is tab-stoppable), arrow keys move the
// selection.

type Segment = { kind: LayoutKind; label: string; title: string };

const SEGMENTS: Segment[] = [
  { kind: "source", label: "Source", title: "Source only" },
  { kind: "split", label: "Split", title: "Editor and preview side by side" },
  { kind: "preview", label: "Preview", title: "Preview only" },
];

function SegmentIcon(props: { kind: LayoutKind }) {
  // 14×14 line icons, currentColor. Source: text lines. Split: two panes.
  // Preview: a single filled pane.
  return (
    <Show when={props.kind === "source"} fallback={
      <Show when={props.kind === "split"} fallback={
        <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
          <rect x="1.5" y="2.5" width="11" height="9" rx="1.5" fill="currentColor" opacity="0.9" />
        </svg>
      }>
        <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
          <rect x="1.5" y="2.5" width="11" height="9" rx="1.5" stroke="currentColor" stroke-width="1.2" fill="none" />
          <line x1="7" y1="2.5" x2="7" y2="11.5" stroke="currentColor" stroke-width="1.2" />
        </svg>
      </Show>
    }>
      <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
        <line x1="2.5" y1="4" x2="11.5" y2="4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
        <line x1="2.5" y1="7" x2="11.5" y2="7" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
        <line x1="2.5" y1="10" x2="8.5" y2="10" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
      </svg>
    </Show>
  );
}

export default function PreviewLayoutToggle() {
  const win = useWindow();
  const activeBuffer = useActiveBuffer();

  const renderable = createMemo(() => {
    const buf = activeBuffer();
    return buf ? rendererRegistry.hasRenderer(contentTypeForBuffer(buf)) : false;
  });

  const currentKind = createMemo<LayoutKind>(() => {
    const buf = activeBuffer();
    return buf ? win.layout.get(buf.id).kind : "source";
  });

  function select(kind: LayoutKind) {
    const buf = activeBuffer();
    if (!buf) return;
    const current = win.layout.get(buf.id);
    let next: LayoutMode;
    if (kind === "split") {
      // Preserve an existing ratio; otherwise default to 50/50.
      next = current.kind === "split" ? current : defaultSplit();
    } else {
      next = { kind };
    }
    win.layout.set(buf.id, buf.source_path, next);
  }

  function onKeyDown(e: KeyboardEvent) {
    const idx = SEGMENTS.findIndex((s) => s.kind === currentKind());
    let nextIdx: number | null = null;
    if (e.key === "ArrowRight" || e.key === "ArrowDown") nextIdx = (idx + 1) % SEGMENTS.length;
    else if (e.key === "ArrowLeft" || e.key === "ArrowUp") nextIdx = (idx - 1 + SEGMENTS.length) % SEGMENTS.length;
    else if (e.key === "Home") nextIdx = 0;
    else if (e.key === "End") nextIdx = SEGMENTS.length - 1;
    if (nextIdx === null) return;
    e.preventDefault();
    select(SEGMENTS[nextIdx].kind);
  }

  return (
    <Show when={renderable()}>
      <div class="layout-toggle" role="radiogroup" aria-label="Preview layout" onKeyDown={onKeyDown}>
        <For each={SEGMENTS}>
          {(seg) => {
            const isActive = () => currentKind() === seg.kind;
            return (
              <button
                type="button"
                class="layout-toggle-seg"
                classList={{ "is-active": isActive() }}
                role="radio"
                aria-checked={isActive()}
                aria-label={seg.title}
                title={seg.title}
                tabIndex={isActive() ? 0 : -1}
                onClick={() => select(seg.kind)}
              >
                <SegmentIcon kind={seg.kind} />
                <span class="layout-toggle-label">{seg.label}</span>
              </button>
            );
          }}
        </For>
      </div>
    </Show>
  );
}
