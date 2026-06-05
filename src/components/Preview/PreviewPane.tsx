import { Show, createSignal, createEffect, on, onCleanup, onMount } from "solid-js";
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
  const [state, setState] = createSignal<PreviewState>("rendering");
  const [warnings, setWarnings] = createSignal<string[]>([]);
  const [message, setMessage] = createSignal("");
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;

  // Cache-busting query param forces the iframe to reload fresh HTML; the
  // protocol parser discards the query, so the handler still keys on the id.
  const src = () => `writ-preview://document/${props.buffer.id}?v=${renderVersion()}`;
  const hasRendered = () => renderVersion() > 0;

  async function doRender(force: boolean) {
    const text = win.editor.currentText();
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
      const result = await win.preview.render(props.buffer.id, props.contentType, text);
      if (result.kind === "rendered") {
        setWarnings(result.parser_warnings);
        setState("ok");
        setRenderVersion((v) => v + 1);
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

  onMount(() => {
    void doRender(true);
  });

  // Debounced live re-render on edits.
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
          when={hasRendered()}
          fallback={<div class="preview-pane-empty">rendering…</div>}
        >
          <iframe
            class="preview-frame"
            src={src()}
            title={`Preview of ${props.buffer.title}`}
          />
        </Show>
      </Show>
      <PreviewStatusChip state={state()} warnings={warnings()} message={message()} />
    </div>
  );
}
