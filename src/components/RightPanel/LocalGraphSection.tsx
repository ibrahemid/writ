import { Show, createMemo, onCleanup } from "solid-js";
import PanelSection from "./PanelSection";
import GraphCanvas from "../Graph/GraphCanvas";
import { useWindow } from "../WindowProvider/WindowProvider";
import { noteFactsStore } from "../../stores/global/note-facts";
import { neighbourhoodOf } from "../../lib/graph/neighbourhood";

interface Props {
  /** The note the panel is showing. */
  path: string;
}

/**
 * The notes one link away from the open one, drawn.
 *
 * The neighbourhood is cut out of the folder's graph rather than built from
 * the note's own two lists, so a link between two of the neighbours is drawn
 * too and a note's radius can say how connected it is. Ambiguous links are
 * not in that graph at all: the index resolves none of them (ADR-034), and a
 * line to a guess is the one thing a drawing must not add.
 *
 * A note with nothing around it renders no section, not an empty one.
 */
export default function LocalGraphSection(props: Props) {
  const win = useWindow();
  const graph = noteFactsStore.graph();
  // A panel that has stopped showing the drawing stops the folder graph being
  // re-read on every change on disk.
  onCleanup(() => noteFactsStore.releaseGraph());

  const near = createMemo(() => {
    const rows = graph();
    return neighbourhoodOf(rows.nodes, rows.edges, props.path);
  });

  return (
    <Show when={near().nodes.length > 0}>
      <PanelSection section="graph" heading="Nearby notes">
        <GraphCanvas
          nodes={near().nodes}
          edges={near().edges}
          focusPath={props.path}
          onOpen={(path) => void win.tabs.openFile(path).catch(() => null)}
        />
      </PanelSection>
    </Show>
  );
}
