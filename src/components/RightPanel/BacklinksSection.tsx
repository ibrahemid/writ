import { For, Show } from "solid-js";
import PanelSection from "./PanelSection";
import { useWindow } from "../WindowProvider/WindowProvider";
import { backlinksStore, type Backlink } from "../../stores/global/backlinks";
import { basename, dirname } from "../../lib/path";

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
/** What a note is called, ending in a note extension the link never writes. */
const NOTE_EXTENSION = /\.(md|markdown|mdown|mkd|txt)$/i;

/**
 * A candidate note, by its folder and its name.
 *
 * The name alone cannot tell an ambiguity apart: the two notes are ambiguous
 * because they are called the same thing. The folder is what separates them,
 * so it is what the marker shows.
 */
function candidateName(path: string): string {
  const name = basename(path).replace(NOTE_EXTENSION, "");
  const parent = dirname(path);
  const folder = parent === path ? "" : basename(parent);
  return folder === "" ? name : `${folder}/${name}`;
}

/** What else the link could mean, named. */
function ambiguityMarker(candidates: string[]): string {
  const names = candidates.map(candidateName);
  if (names.length === 0) return "Could name another note";
  if (names.length === 1) return `Could also mean ${names[0]}`;
  return `Could also mean ${names.slice(0, -1).join(", ")} or ${names[names.length - 1]}`;
}

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
