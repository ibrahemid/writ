import { Show, createSignal, createEffect, on, onCleanup } from "solid-js";
import type { BufferDocument } from "../../types/buffer";
import { configStore } from "../../stores/global/config";
import { useWindow } from "../WindowProvider/WindowProvider";
import PreviewStatusChip, { type PreviewState } from "./PreviewStatusChip";
import "./preview-chrome.css";

interface Props {
  buffer: BufferDocument;
  contentType: string;
}

const MB = 1024 * 1024;

export default function PreviewPane(props: Props) {
  const win = useWindow();
  const [renderVersion, setRenderVersion] = createSignal(0);
  // The iframe loads exactly this src. It advances only when a render for
  // THIS buffer's id succeeds, so it never points at the incoming id before
  // that id's HTML is cached: a tab switch keeps showing the outgoing buffer
  // until the incoming one is ready, never a stale/empty/wrong-buffer flash
  // (#97). It is also never reset to null once set, so the iframe element is
  // never torn down and recreated — only navigated — which is what keeps the
  // macOS webview from freezing (#124).
  const [renderedSrc, setRenderedSrc] = createSignal<string | null>(null);
  const [state, setState] = createSignal<PreviewState>("rendering");
  const [warnings, setWarnings] = createSignal<string[]>([]);
  const [message, setMessage] = createSignal("");
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  let lastRenderedText: string | null = null;

  async function doRender(force: boolean) {
    // Coordination guard: never render this pane's buffer id with another
    // buffer's live text. During a tab switch props.buffer.id flips reactively
    // while the editor is still mid-load on the outgoing buffer; rendering then
    // would cache the wrong buffer's HTML under this id (#97 cache pollution).
    // A null loaded id means the editor has not published a buffer yet (or is
    // stubbed in tests) — allow it; the size/attribution invariant still holds.
    const loadedId = win.editor.currentBufferId();
    if (loadedId !== null && loadedId !== props.buffer.id) return;

    // Capture the target id once: the pane is persistent, so props.buffer.id
    // can change under us across the await below. Both the render call and the
    // resulting src must use this captured id, never a re-read of the prop.
    const bufferId = props.buffer.id;
    const text = win.editor.currentText();
    // Skip a debounced re-render that would reproduce the already-cached HTML
    // (a no-op edit, or the load-induced currentText change right after a
    // switch already rendered by the buffer-id effect). force always renders.
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
      const result = await win.preview.render(bufferId, props.contentType, text);
      // The active buffer may have switched while this render was in flight.
      // Discard the result: committing it would point the iframe at this id's
      // slot under the wrong active buffer (the #97 flash via the completion
      // race) and poison lastRenderedText. Rust has already cached the HTML
      // under bufferId, so switching back force-renders from the cache.
      if (props.buffer.id !== bufferId) return;
      if (result.kind === "rendered") {
        setWarnings(result.parser_warnings);
        setState("ok");
        lastRenderedText = text;
        setRenderVersion((prev) => prev + 1);
        const v = renderVersion();
        // Cache-busting query param forces the iframe to reload fresh HTML; the
        // protocol parser discards the query, so the handler still keys on the
        // id. Pointing at the id only now (post-success) is what avoids the
        // version-stale retarget that caused the flash.
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

  // Full render whenever the editor publishes this pane's buffer as the loaded
  // one: the initial mount (once the editor reports a matching id) and every
  // tab switch back to this buffer. Not deferred so the initial value drives
  // the first render. force=true bypasses the text-dedup so an identical-text
  // sibling buffer still renders on switch.
  createEffect(
    on(
      () => win.editor.currentBufferId(),
      (id) => {
        if (id === props.buffer.id) void doRender(true);
      },
    ),
  );

  // Debounced live re-render on edits to the loaded buffer.
  createEffect(
    on(
      () => win.editor.currentText(),
      () => {
        if (debounceTimer) clearTimeout(debounceTimer);
        const delay = configStore.config().preview.debounce_ms;
        debounceTimer = setTimeout(() => void doRender(false), delay);
      },
      { defer: true },
    ),
  );

  // Cmd+R force refresh (bypasses the size-threshold debounce gate).
  createEffect(
    on(
      () => win.preview.forceRefreshToken(),
      () => void doRender(true),
      { defer: true },
    ),
  );

  onCleanup(() => {
    if (debounceTimer) clearTimeout(debounceTimer);
    win.preview.close(props.buffer.id);
  });

  return (
    <div class="preview-pane">
      <Show
        when={state() !== "too_large"}
        fallback={
          <div class="preview-pane-empty">
            document too large to preview — use source view
          </div>
        }
      >
        <Show
          when={renderedSrc()}
          fallback={<div class="preview-pane-empty">rendering…</div>}
        >
          {(currentSrc) => (
            <iframe
              class="preview-frame"
              src={currentSrc()}
              title={`Preview of ${props.buffer.title}`}
            />
          )}
        </Show>
      </Show>
      <PreviewStatusChip state={state()} warnings={warnings()} message={message()} />
    </div>
  );
}
