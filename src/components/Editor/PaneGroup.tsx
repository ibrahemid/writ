import { Show, createEffect, createMemo, untrack, type JSX } from "solid-js";
import EditorInstance from "./EditorInstance";
import NoteDownloading from "./NoteDownloading";
import StartSurface from "./StartSurface";
import TabBar from "./TabBar";
import PreviewSplit from "../Preview/PreviewSplit";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { useWindow } from "../WindowProvider/WindowProvider";
import type { PaneId } from "../../lib/pane-layout";
import type { BufferDocument } from "../../types/buffer";
import "./PaneGroup.css";

interface PaneProps {
  pane: PaneId;
  buffer: BufferDocument | null;
  /** Whether the pane carries its own tab strip: only while both show. */
  strip: boolean;
  style?: JSX.CSSProperties;
}

function EditorPane(props: PaneProps) {
  const win = useWindow();
  const buffer = () => props.buffer;
  const focusHere = () => win.panes.focus(props.pane);

  return (
    <div
      class="editor-pane"
      classList={{ "is-focused": props.strip && win.panes.focused() === props.pane }}
      style={props.style}
      data-pane={props.pane}
      onPointerDown={focusHere}
      onFocusIn={focusHere}
    >
      <Show when={props.strip}>
        <TabBar pane={props.pane} />
      </Show>
      <div class="editor-pane-body">
        <Show
          when={buffer()}
          fallback={
            <Show
              when={props.pane === "left" && win.downloads.selected()}
              fallback={<StartSurface />}
            >
              {(download) => (
                <NoteDownloading
                  download={download()}
                  onRetry={() => void win.downloads.retry(download().path)}
                  onDismiss={() => void win.downloads.dismiss(download().path)}
                />
              )}
            </Show>
          }
        >
          {(buf) => <EditorInstance buffer={buf()} pane={props.pane} />}
        </Show>
      </div>
    </div>
  );
}

interface Props {
  /** The focused pane's tab, which the preview beside the panes shows too. */
  focusedBuffer: BufferDocument | null;
}

/**
 * The editor's one or two panes, side by side (ADR-056). The left pane is
 * always there, so its editor is never rebuilt when a split opens or closes;
 * the right one and the divider come with the split.
 */
export default function PaneGroup(props: Props) {
  const win = useWindow();
  let groupRef: HTMLDivElement | undefined;
  const split = () => win.panes.isSplit();
  // The focused pane shows the tab it is given; the other one looks its tab
  // up. Both name the same row, so focus moving reloads nothing.
  const otherBuffer = createMemo(() => {
    const id = win.panes.activeIn(win.panes.focused() === "left" ? "right" : "left");
    if (id === null) return null;
    return bufferRegistry.activeTabs().find((b) => b.id === id) ?? null;
  });
  const bufferOf = (pane: PaneId) => (win.panes.focused() === pane ? props.focusedBuffer : otherBuffer());

  // Tabs closed by anything other than the tab store (a folder moved, a file
  // removed) leave the panes too.
  createEffect(() => {
    const open = bufferRegistry.activeTabs().map((b) => b.id);
    untrack(() => win.panes.settle(open));
  });

  const leftStyle = (): JSX.CSSProperties =>
    split()
      ? { "flex-grow": "0", "flex-shrink": "0", "flex-basis": `${win.panes.ratio() * 100}%` }
      : { "flex-grow": "1", "flex-basis": "0" };

  return (
    <div class="pane-group" ref={groupRef}>
      <EditorPane pane="left" buffer={bufferOf("left")} strip={split()} style={leftStyle()} />
      <Show when={split()}>
        <PreviewSplit
          orientation="vertical"
          ratio={win.panes.ratio()}
          container={() => groupRef}
          onResize={(ratio) => win.panes.setRatio(ratio, false)}
          onCommit={() => win.panes.setRatio(win.panes.ratio(), true)}
          label="Resize panes"
        />
        <EditorPane pane="right" buffer={bufferOf("right")} strip />
      </Show>
    </div>
  );
}
