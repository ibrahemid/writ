import { Show, createMemo, createSignal } from "solid-js";
import EdgeResizer from "../Resizer/EdgeResizer";
import BacklinksSection from "./BacklinksSection";
import OutlineSection from "./OutlineSection";
import PropertiesSection from "./PropertiesSection";
import { useWindow } from "../WindowProvider/WindowProvider";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { noteFactsStore } from "../../stores/global/note-facts";
import {
  configStore,
  PANEL_WIDTH_DEFAULT,
  PANEL_WIDTH_MAX,
  PANEL_WIDTH_MIN,
} from "../../stores/global/config";
import "./RightPanel.css";

/**
 * The panel beside the note: what links to it, its headings, its properties.
 *
 * Every section reads one call. `factsFor` is asked for here, once, and the
 * accessor is handed down, so a change on disk re-reads the note once and
 * three sections follow it rather than each asking again.
 *
 * A section with nothing in it renders nothing at all — no heading, no line
 * saying it is empty. A note with none of the three leaves the panel showing
 * its ground and its edge, which is the honest answer.
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

  // One read for the whole panel: the outline and the properties are two
  // views of one answer, not two calls (ADR-036).
  const facts = createMemo(() => {
    const note = openNote();
    return note === null ? null : noteFactsStore.factsFor(note.path);
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
      inert={!win.rightPanel.isOpen()}
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
        onReset={() => configStore.setPanelWidth(PANEL_WIDTH_DEFAULT)}
      />
      <div class="right-panel-inner">
        <div class="right-panel-scroll">
          <Show when={openNote()}>
            {(note) => (
              <>
                <BacklinksSection path={note().path} />
                <Show when={facts()}>
                  {(read) => (
                    <>
                      <OutlineSection facts={read()} bufferId={note().id} />
                      <PropertiesSection facts={read()} />
                    </>
                  )}
                </Show>
              </>
            )}
          </Show>
        </div>
      </div>
    </aside>
  );
}
