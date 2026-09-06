import { For, Show } from "solid-js";
import PanelSection from "./PanelSection";
import { useWindow } from "../WindowProvider/WindowProvider";
import { backlinksStore, type Backlink } from "../../stores/global/backlinks";
import { ambiguityMarker } from "../../lib/note-target";

interface Props {
  /** The note the panel is showing. */
  path: string;
}

/**
 * The notes that link to the open one.
 *
 * A link whose target names this note and another one is carried as what it
 * is. The index reports the ambiguity and picks neither note (ADR-034), and
 * the row names the notes it might mean instead rather than reading as a
 * settled link.
 */
export default function BacklinksSection(props: Props) {
  const win = useWindow();
  const rows = () => backlinksStore.backlinksFor(props.path)();

  async function open(row: Backlink) {
    const doc = await win.tabs.openFile(row.from_path).catch(() => null);
    if (doc) win.editor.requestReveal(doc.id, row.line);
  }

  return (
    <Show when={rows().length > 0}>
      <PanelSection section="backlinks" heading="Links to this note">
        <ul class="right-panel-list">
          <For each={rows()}>
            {(row) => (
              <li>
                <button type="button" class="right-panel-row" onClick={() => void open(row)}>
                  <span class="right-panel-row-name">{row.from_name}</span>
                  <Show when={row.context !== ""}>
                    <span class="right-panel-row-context">{row.context}</span>
                  </Show>
                  <Show when={row.certainty === "ambiguous"}>
                    <span class="right-panel-row-marker">
                      {ambiguityMarker(row.candidates)}
                    </span>
                  </Show>
                </button>
              </li>
            )}
          </For>
        </ul>
      </PanelSection>
    </Show>
  );
}
