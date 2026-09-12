import { Show } from "solid-js";
import { abbreviateTitle } from "../../lib/buffer-name";
import Icon, { type IconName } from "../Icon/Icon";
import Tooltip from "../Tooltip/Tooltip";
import "./TabItem.css";

interface Props {
  /** The row's visible name. Not a `title` attribute: rows carry a Tooltip. */
  label: string;
  icon?: IconName;
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
      class="tab-item"
      role="button"
      tabIndex={0}
      onClick={props.onClick}
      onKeyDown={handleKeyDown}
    >
      <Show when={props.icon}>
        {(name) => <Icon name={name()} />}
      </Show>
      <Tooltip label={props.label} requiresTruncation>
        <span class="tab-item-title">{abbreviateTitle(props.label)}</span>
      </Tooltip>
      {props.secondary && <span class="tab-item-secondary">{props.secondary}</span>}
      {props.trailing && <span class="tab-item-trailing">{props.trailing}</span>}
      <div class="tab-item-actions">
        {props.onRestore && (
          <Tooltip label="Restore tab">
            <button
              type="button"
              class="tab-item-action"
              aria-label="Restore tab"
              onClick={(e) => {
                e.stopPropagation();
                props.onRestore!();
              }}
            >
              <Icon name="arrow-u-down-left" size={14} />
            </button>
          </Tooltip>
        )}
        {props.onClose && (
          <Tooltip label="Close tab">
            <button
              type="button"
              class="tab-item-action tab-item-close"
              aria-label="Close tab"
              onClick={(e) => {
                e.stopPropagation();
                props.onClose!();
              }}
            >
              <Icon name="x" size={14} />
            </button>
          </Tooltip>
        )}
      </div>
    </div>
  );
}
