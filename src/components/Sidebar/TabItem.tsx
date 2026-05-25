import { abbreviateTitle } from "../../lib/buffer-name";
import "./TabItem.css";

interface Props {
  title: string;
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
