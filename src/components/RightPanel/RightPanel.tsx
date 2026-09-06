import { Show, createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import EdgeResizer from "../Resizer/EdgeResizer";
import BacklinksSection from "./BacklinksSection";
import LinksSection from "./LinksSection";
import LocalGraphSection from "./LocalGraphSection";
import OutlineSection from "./OutlineSection";
import PropertiesSection from "./PropertiesSection";
import { useWindow } from "../WindowProvider/WindowProvider";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { noteFactsStore } from "../../stores/global/note-facts";
import { backlinksStore } from "../../stores/global/backlinks";
import {
  PANEL_WIDTH_DEFAULT,
  PANEL_WIDTH_MAX,
  PANEL_WIDTH_MIN,
} from "../../stores/global/config";
import "./RightPanel.css";

/**
 * The panel beside the note: its headings, the notes it links to, the notes
 * that link to it, those notes drawn, and its properties.
 *
 * The two lists of links sit above the drawing, and every note the drawing
 * holds is a row in one of them: the drawing is a second way to reach a note
 * and never the only one.
 *
 * `factsFor` is asked for here, once, and the accessor is handed down, so a
 * change on disk re-reads the note once and the sections that read it follow
 * that rather than each asking again.
 *
 * A section with nothing in it renders nothing at all — no heading, no line
 * saying it is empty. A note with none of them leaves the panel showing its
 * ground and its edge, which is the honest answer.
 */
export default function RightPanel() {
  const win = useWindow();

  // Non-null only while a drag is in flight: the edge follows the pointer
  // without a disk write per frame, and release commits the settled width.
  const [dragWidth, setDragWidth] = createSignal<number | null>(null);
  const width = () => dragWidth() ?? win.rightPanel.width();

  /**
   * The note the panel is showing: its buffer and its file, read off one tab
   * so a jump to a heading and the facts it came from cannot name two
   * different notes.
   *
   * A closed panel is showing none. A tab with no file behind it — a note
   * that has never been written — has nothing in the index either.
   */
  const openNote = createMemo(() => {
    if (!win.rightPanel.isOpen()) return null;
    const id = win.tabs.activeTabId();
    if (!id) return null;
    const doc = bufferRegistry.activeTabs().find((tab) => tab.id === id);
    if (!doc?.source_path) return null;
    return { id: doc.id, path: doc.source_path };
  });

  // The file alone, so a tab changing for any other reason does not read the
  // same note again.
  const openPath = createMemo(() => openNote()?.path ?? null);

  // One read for the whole panel: the outline and the properties are two
  // views of one answer, not two calls (ADR-036).
  const facts = createMemo(() => {
    const path = openPath();
    return path === null ? null : noteFactsStore.factsFor(path);
  });

  // A note the panel has stopped showing stops being followed. Without this
  // every note visited while the panel was open stays held, so one change on
  // disk costs a read per note visited rather than a read for the note shown.
  createEffect(() => {
    const path = openPath();
    if (path === null) return;
    onCleanup(() => {
      noteFactsStore.release(path);
      backlinksStore.release(path);
    });
  });

  // `inert` is presence-based, so it is set as an attribute rather than left
  // to the property: a closed panel keeps its rows out of the tab order at
  // zero width, and an open one carries no attribute at all.
  let panel: HTMLElement | undefined;
  createEffect(() => {
    panel?.toggleAttribute("inert", !win.rightPanel.isOpen());
  });

  return (
    <aside
      class="right-panel"
      classList={{
        "is-open": win.rightPanel.isOpen(),
        "is-resizing": dragWidth() !== null,
      }}
      style={{ "--writ-panel-live-width": `${width()}px` }}
      aria-label="Connections"
      aria-hidden={win.rightPanel.isOpen() ? undefined : "true"}
      ref={panel}
    >
      <EdgeResizer
        class="right-panel-resizer"
        label="Connections width"
        width={() => win.rightPanel.width()}
        min={PANEL_WIDTH_MIN}
        max={PANEL_WIDTH_MAX}
        direction={-1}
        onDrag={setDragWidth}
        onCommit={(next) => win.rightPanel.setWidth(next)}
        onReset={() => win.rightPanel.setWidth(PANEL_WIDTH_DEFAULT)}
      />
      <div class="right-panel-inner">
        <div class="right-panel-scroll">
          <Show when={openNote()}>
            {(note) => (
              <>
                <Show when={facts()}>
                  {(read) => <OutlineSection facts={read()} bufferId={note().id} />}
                </Show>
                <LinksSection path={note().path} />
                <BacklinksSection path={note().path} />
                <LocalGraphSection path={note().path} />
                <Show when={facts()}>{(read) => <PropertiesSection facts={read()} />}</Show>
              </>
            )}
          </Show>
        </div>
      </div>
    </aside>
  );
}
