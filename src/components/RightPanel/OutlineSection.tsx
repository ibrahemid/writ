import { For, Show, type Accessor } from "solid-js";
import PanelSection from "./PanelSection";
import { useWindow } from "../WindowProvider/WindowProvider";
import type { NoteFacts, NoteHeading } from "../../stores/global/note-facts";

interface Props {
  facts: Accessor<NoteFacts>;
  /** The open note's buffer, which is what a jump to a line names. */
  bufferId: string;
}

/** How far one heading level sits past the one above it. */
const INDENT_PX = 16;

/**
 * The open note's headings, in the order they are written.
 *
 * A click moves the caret through the editor store, which is the same path a
 * search result and a `[[Note#Heading]]` take. Nothing here reaches for the
 * document or for CodeMirror.
 */
export default function OutlineSection(props: Props) {
  const win = useWindow();
  const headings = () => props.facts().headings;

  function indentOf(heading: NoteHeading): string {
    return `${Math.max(0, heading.level - 1) * INDENT_PX}px`;
  }

  return (
    <Show when={headings().length > 0}>
      <PanelSection section="outline" heading="Outline">
        <ul class="right-panel-list">
          <For each={headings()}>
            {(heading) => (
              <li>
                <button
                  type="button"
                  class="right-panel-row right-panel-heading"
                  style={{ "padding-left": `calc(var(--writ-space-3) + ${indentOf(heading)})` }}
                  data-level={heading.level}
                  onClick={() => win.editor.requestReveal(props.bufferId, heading.line)}
                >
                  <span class="right-panel-row-name">{heading.text}</span>
                </button>
              </li>
            )}
          </For>
        </ul>
      </PanelSection>
    </Show>
  );
}
