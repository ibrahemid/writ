import { For, Show, createEffect, createSignal } from "solid-js";
import PanelSection from "./PanelSection";
import { useWindow } from "../WindowProvider/WindowProvider";
import { noteFactsStore, type NoteLink } from "../../stores/global/note-facts";
import { linkStore, type LinkResolution } from "../../stores/global/link";
import { ambiguityMarker, targetName } from "../../lib/note-target";

interface Props {
  /** The note the panel is showing. */
  path: string;
}

/** One outgoing link, as the panel lists it. */
interface Row {
  /** The note it reached, or null when it reached none or more than one. */
  path: string | null;
  name: string;
  /** The notes the target could have meant. Empty unless it named several. */
  candidates: string[];
}

/**
 * The notes this note links to.
 *
 * One row per note, however many times the note links to it, in the order the
 * links are written. A target the index could not settle on is listed as what
 * was written rather than dropped: a link to a note that is not there yet is
 * still something the note says, and a target naming two notes says which two
 * (ADR-034). Neither opens anything, because there is nothing settled to open.
 */
export default function LinksSection(props: Props) {
  const win = useWindow();
  const read = () => noteFactsStore.factsFor(props.path)();
  const [resolutions, setResolutions] = createSignal<ReadonlyMap<string, LinkResolution>>(
    new Map(),
  );

  /** The targets the index picked no note for, each asked about once. */
  const unsettled = (): string[] => {
    const targets: string[] = [];
    for (const link of read().links) {
      if (link.to_path === null && !targets.includes(link.to_target)) targets.push(link.to_target);
    }
    return targets;
  };

  // Whether a target named nothing or named several notes is not in the facts
  // the index hands back, and it is what decides how the row reads. The answers
  // land one at a time; a target still being asked about reads as naming
  // nothing, which is what it looks like until something says otherwise.
  createEffect(() => {
    const from = props.path;
    const targets = unsettled();
    setResolutions(new Map());
    for (const target of targets) {
      void linkStore.resolveNoteLink(from, target).then((resolution) => {
        if (from !== props.path) return;
        setResolutions((held) => new Map(held).set(target, resolution));
      });
    }
  });

  const rows = (): Row[] => {
    const settled = resolutions();
    const listed = new Set<string>();
    const out: Row[] = [];
    for (const link of read().links) {
      const key = link.to_path ?? link.to_target;
      if (listed.has(key)) continue;
      listed.add(key);
      out.push(rowFor(link, settled.get(link.to_target)));
    }
    return out;
  };

  function rowFor(link: NoteLink, resolution: LinkResolution | undefined): Row {
    if (link.to_path !== null) {
      return { path: link.to_path, name: targetName(link.to_path), candidates: [] };
    }
    const candidates = resolution?.status === "ambiguous" ? resolution.candidates : [];
    return { path: null, name: link.to_target, candidates };
  }

  async function open(path: string) {
    await win.tabs.openFile(path).catch(() => null);
  }

  return (
    <Show when={rows().length > 0}>
      <PanelSection section="links" heading="Links">
        <ul class="right-panel-list">
          <For each={rows()}>
            {(row) => (
              <li>
                <Show
                  when={row.path}
                  fallback={
                    <span class="right-panel-row right-panel-row-flat">
                      <span class="right-panel-row-name right-panel-row-unsettled">{row.name}</span>
                      <Show when={row.candidates.length > 0}>
                        <span class="right-panel-row-marker">
                          {ambiguityMarker(row.candidates)}
                        </span>
                      </Show>
                    </span>
                  }
                >
                  {(path) => (
                    <button
                      type="button"
                      class="right-panel-row"
                      onClick={() => void open(path())}
                    >
                      <span class="right-panel-row-name">{row.name}</span>
                    </button>
                  )}
                </Show>
              </li>
            )}
          </For>
        </ul>
      </PanelSection>
    </Show>
  );
}
