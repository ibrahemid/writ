import { Show } from "solid-js";
import type { ConflictCopyKind } from "../../types/workspace";
import "./ConflictCopyBadge.css";

/**
 * Marks a file that is a second copy of another note.
 *
 * A copy is listed like any other file, so without the mark the two names sit
 * side by side with nothing to say which one is the extra. Naming the service
 * that made it is what tells the user where to look.
 */
export function ConflictCopyBadge(props: {
  kind: ConflictCopyKind | null;
  provider?: string | null;
}) {
  const label = () => (props.kind === "writ" ? "Writ copy" : "Sync copy");

  const title = () => {
    if (props.kind === "writ") {
      return "Writ kept this copy when the file changed on disk while it was open.";
    }
    const service = props.provider ?? "Your sync service";
    return `${service} kept this copy when the note was edited in two places.`;
  };

  return (
    <Show when={props.kind}>
      <span class="file-tree-copy-badge" title={title()}>
        {label()}
      </span>
    </Show>
  );
}
