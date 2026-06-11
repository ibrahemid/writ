import { Show, createSignal, createEffect, on, onMount, onCleanup, untrack } from "solid-js";
import type { BufferDocument } from "../../types/buffer";
import { configStore } from "../../stores/global/config";
import { useWindow } from "../WindowProvider/WindowProvider";
import { createPreviewBridge } from "../../lib/preview-bridge";
import { createPreviewSearchController } from "../../editor/search/preview-search-controller";
import { findStore } from "../../stores/global/find-store";
import PreviewStatusChip, { type PreviewState } from "./PreviewStatusChip";
import "./preview-chrome.css";

interface Props {
  buffer: BufferDocument | null;
  contentType: string | null;
  // Whether this pane should show a live preview. When false the iframe is
  // navigated to a blank document but kept mounted — the element is NEVER
  // removed, because tearing down a loaded writ-preview:// iframe hard-freezes
  // the macOS webview (#124). Every preview transition is an src navigation.
  isActive: boolean;
  // Whether the editor is shown alongside this pane (split layout on a
  // renderable buffer). Gates the bidirectional scroll sync — only meaningful
  // when both surfaces are visible.
  isSplit?: boolean;
}

const MB = 1024 * 1024;
// Same-origin near-empty document the iframe parks on when no preview is
// active. Parking here instead of unmounting the element is what keeps the
// teardown freeze from ever being reachable.
const BLANK_SRC = "writ-preview://chrome/blank";

export default function PreviewPane(props: Props) {
  const win = useWindow();
  const [renderVersion, setRenderVersion] = createSignal(0);
  // The iframe always loads this; it is never null, so the element is created
  // once and only ever navigated. It advances to a document URL only on a
  // successful, correctly-attributed render (so a tab switch keeps showing the
  // outgoing buffer until the incoming one is ready — no #97 flash) and falls
  // back to BLANK_SRC when the pane goes inactive.
  const [renderedSrc, setRenderedSrc] = createSignal<string>(BLANK_SRC);
  const [state, setState] = createSignal<PreviewState>("rendering");
  const [warnings, setWarnings] = createSignal<string[]>([]);
  const [message, setMessage] = createSignal("");
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  let lastRenderedText: string | null = null;
  let iframeEl: HTMLIFrameElement | undefined;

  function editorScroller(): HTMLElement | undefined {
    return win.editor.getView()?.scrollDOM;
  }

  // Parent half of the preview bridge: mirrors scroll between the editor and
  // the cross-origin preview iframe and restores position across reloads. The
  // iframe runtime is src-tauri/assets/preview/bridge.js.
  const bridge = createPreviewBridge({
    isSplit: () => props.isSplit === true,
    getEditorMetrics: () => {
      const el = editorScroller();
      if (!el) return null;
      return { top: el.scrollTop, range: el.scrollHeight - el.clientHeight };
    },
    setEditorScrollTop: (top) => {
      const el = editorScroller();
      if (el) el.scrollTop = top;
    },
    postScrollTo: (fraction) => postToIframe({ type: "scrollTo", fraction }),
  });

  function postToIframe(payload: Record<string, unknown>): void {
    iframeEl?.contentWindow?.postMessage(
      { source: "writ-preview", dir: "down", ...payload },
      "*",
    );
  }

  // In-preview find: posts commands to the iframe runtime and folds its async
  // results into a snapshot the find overlay reads. Registered as the find
  // target only while the preview is shown alone (the editor hidden).
  const search = createPreviewSearchController({
    post: (command) => postToIframe(command),
    onUpdate: () => findStore.refresh(),
  });

  function onWindowMessage(e: MessageEvent) {
    if (e.source !== iframeEl?.contentWindow) return;
    const d = e.data;
    if (!d || d.source !== "writ-preview" || d.dir !== "up") return;
    if (d.type === "ready") {
      bridge.onIframeMessage({ type: "ready" });
      search.reapply(); // a reload dropped any highlights — restore them
    } else if (d.type === "scroll" && typeof d.fraction === "number") {
      bridge.onIframeMessage({ type: "scroll", fraction: d.fraction });
    } else if (d.type === "findResult") {
      search.applyResult({
        current: d.current ?? 0,
        total: d.total ?? 0,
        capped: d.capped ?? false,
        ticks: Array.isArray(d.ticks) ? d.ticks : [],
      });
    }
  }

  async function doRender(force: boolean) {
    const buffer = props.buffer;
    const contentType = props.contentType;
    if (!props.isActive || !buffer || !contentType) return;

    // Coordination guard: never render this pane's buffer id with another
    // buffer's live text. During a tab switch props.buffer.id flips reactively
    // while the editor is still mid-load on the outgoing buffer (#97). A null
    // loaded id means the editor hasn't published a buffer yet (or is stubbed).
    const loadedId = win.editor.currentBufferId();
    if (loadedId !== null && loadedId !== buffer.id) return;

    // Capture the target id once: the pane is persistent, so props.buffer can
    // change under us across the await below.
    const bufferId = buffer.id;
    const text = win.editor.currentText();
    // Skip a debounced re-render that would reproduce the already-cached HTML.
    if (!force && text === lastRenderedText) return;

    const cfg = configStore.config().preview;
    const bytes = new TextEncoder().encode(text).length;

    if (bytes > cfg.render_refuse_threshold_mb * MB) {
      setState("too_large");
      return;
    }
    if (!force && bytes > cfg.live_render_threshold_mb * MB) {
      setState("manual");
      return;
    }

    setState("rendering");
    try {
      const result = await win.preview.render(bufferId, contentType, text);
      // The pane may have gone inactive, or the active buffer switched, while
      // this render was in flight. Discard: committing would mis-target the
      // iframe (the #97 flash via the completion race) and poison the dedup.
      if (!props.isActive || props.buffer?.id !== bufferId) return;
      if (result.kind === "rendered") {
        setWarnings(result.parser_warnings);
        setState("ok");
        lastRenderedText = text;
        setRenderVersion((prev) => prev + 1);
        const v = renderVersion();
        // Cache-busting query param forces a fresh load; the protocol parser
        // discards the query, so the handler still keys on the id.
        setRenderedSrc(`writ-preview://document/${bufferId}?v=${v}`);
      } else if (result.kind === "no_renderer") {
        setState("error");
        setMessage(`no renderer for ${result.content_type}`);
      } else {
        setState("error");
        setMessage(result.message);
      }
    } catch (err) {
      setState("error");
      setMessage(String(err));
    }
  }

  // Activate / deactivate. On activate (source->split toggle, or a buffer that
  // became renderable) render now if the editor already holds this buffer. On
  // deactivate park on the blank doc — an src navigation, never a teardown.
  createEffect(
    on(
      () => props.isActive,
      (active) => {
        if (active) {
          if (props.buffer && win.editor.currentBufferId() === props.buffer.id) {
            void doRender(true);
          }
        } else {
          if (debounceTimer) clearTimeout(debounceTimer);
          lastRenderedText = null;
          setState("rendering");
          setRenderedSrc(BLANK_SRC);
        }
      },
      { defer: true },
    ),
  );

  // Full render when the editor publishes this pane's buffer as loaded: initial
  // mount and every tab switch back to this buffer. force bypasses the dedup so
  // an identical-text sibling still renders on switch.
  createEffect(
    on(
      () => win.editor.currentBufferId(),
      (id) => {
        if (props.isActive && props.buffer && id === props.buffer.id) void doRender(true);
      },
    ),
  );

  // Debounced live re-render on edits to the loaded buffer.
  createEffect(
    on(
      () => win.editor.currentText(),
      () => {
        if (!props.isActive) return;
        if (debounceTimer) clearTimeout(debounceTimer);
        const delay = configStore.config().preview.debounce_ms;
        debounceTimer = setTimeout(() => void doRender(false), delay);
      },
      { defer: true },
    ),
  );

  // F5 force refresh (bypasses the size-threshold debounce gate).
  createEffect(
    on(
      () => win.preview.forceRefreshToken(),
      () => {
        if (props.isActive) void doRender(true);
      },
      { defer: true },
    ),
  );

  // Bridge messages arrive on the app window; the source check pins them to
  // this pane's iframe (the only writ-preview:// frame).
  onMount(() => {
    window.addEventListener("message", onWindowMessage);
  });

  // (Re)bind the editor scroll listener to the live view, and forget any
  // remembered scroll position, whenever the loaded buffer changes — a tab
  // switch destroys and recreates the EditorView.
  createEffect(
    on(
      () => win.editor.currentBufferId(),
      () => {
        bridge.reset();
        const el = editorScroller();
        if (!el) return;
        const handler = () => bridge.onEditorScroll();
        el.addEventListener("scroll", handler, { passive: true });
        onCleanup(() => el.removeEventListener("scroll", handler));
      },
    ),
  );

  // Own find only when the preview is shown alone — in split the editor is
  // visible and remains the find target. Clear preview highlights when handing
  // find back to the editor.
  createEffect(() => {
    const previewOnly = props.isActive && props.isSplit !== true;
    if (previewOnly) {
      win.preview.registerSearch(search);
    } else {
      search.clear();
      win.preview.registerSearch(null);
    }
    // If find is open across this flip, seed the now-active surface with the
    // current query so it highlights and counts immediately. Untracked so this
    // effect depends only on the layout, not on the find query signals.
    untrack(() => findStore.retarget());
  });

  onCleanup(() => {
    if (debounceTimer) clearTimeout(debounceTimer);
    window.removeEventListener("message", onWindowMessage);
    win.preview.registerSearch(null);
    if (props.buffer) win.preview.close(props.buffer.id);
  });

  return (
    <div class="preview-pane">
      <iframe
        ref={iframeEl}
        class="preview-frame"
        src={renderedSrc()}
        title={props.buffer ? `Preview of ${props.buffer.title}` : "Preview"}
      />
      <Show when={state() === "too_large"}>
        <div class="preview-pane-overlay">
          document too large to preview — use source view
        </div>
      </Show>
      <PreviewStatusChip state={state()} warnings={warnings()} message={message()} />
    </div>
  );
}
