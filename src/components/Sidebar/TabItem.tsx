import { Show } from "solid-js";
import { abbreviateTitle } from "../../lib/buffer-name";
import SaveMarker from "../SaveMarker/SaveMarker";
import "./TabItem.css";

interface Props {
  title: string;
  // Set for a row that stands for an open note, so it can carry the mark for
  // text that is not on disk. A history row stands for a closed one and has
  // nothing to say about saving.
  noteId?: string;
  isActive?: boolean;
  onClick: () => void;
  onClose?: () => void;
  onRestore?: () => void;
  secondary?: string;
  trailing?: string;
}

export default function TabItem(props: Props) {
  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      props.onClick();
    }
  }

  return (
    <div
      class={`tab-item ${props.isActive ? "tab-item-active" : ""}`}
      role="button"
      tabIndex={0}
      onClick={props.onClick}
      onKeyDown={handleKeyDown}
      title={props.title}
    >
      <span class="tab-item-title">{abbreviateTitle(props.title)}</span>
      <Show when={props.noteId}>{(id) => <SaveMarker noteId={id()} />}</Show>
      {props.secondary && <span class="tab-item-secondary">{props.secondary}</span>}
      {props.trailing && <span class="tab-item-trailing">{props.trailing}</span>}
      <div class="tab-item-actions">
        {props.onRestore && (
          <button
            type="button"
            class="tab-item-action"
            aria-label="Restore tab"
            title="Restore"
            onClick={(e) => { e.stopPropagation(); props.onRestore!(); }}
          >
            ↩
          </button>
        )}
        {props.onClose && (
          <button
            type="button"
            class="tab-item-action tab-item-close"
            aria-label="Close tab"
            title="Close"
            onClick={(e) => { e.stopPropagation(); props.onClose!(); }}
          >
            ×
          </button>
        )}
      </div>
    </div>
  );
}
