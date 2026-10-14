import { Show } from "solid-js";
import { abbreviateTitle } from "../../lib/buffer-name";
import { displayFileName } from "../../lib/display-name";
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

/**
 * One row of a sidebar list: a button that opens the row, and its actions as
 * buttons beside it rather than inside it, so a screen reader reaches each
 * control on its own (NV-30). The list around it carries `role="list"`.
 */
export default function TabItem(props: Props) {
  const shown = () => abbreviateTitle(displayFileName(props.label));

  return (
    <div
      class="sidebar-row tab-item"
      classList={{ "has-trailing": Boolean(props.trailing) }}
      role="listitem"
    >
      {/* The ring is drawn on the row around it, so the row reads as focused. */}
      <button type="button" class="tab-item-main" data-writ-focus-silent onClick={props.onClick}>
        <Show when={props.icon}>
          {(name) => <Icon name={name()} />}
        </Show>
        {/* The tip is unconditional once the row shows a stem: the extension it
            drops is the part the reader came to the tip for. */}
        <Tooltip label={props.label} requiresTruncation={shown() === props.label}>
          <span class="tab-item-title">{shown()}</span>
        </Tooltip>
        {props.secondary && <span class="tab-item-secondary">{props.secondary}</span>}
        {props.trailing && <span class="tab-item-trailing">{props.trailing}</span>}
      </button>
      <Show when={props.onRestore || props.onClose}>
        <div class="tab-item-actions">
          {props.onRestore && (
            <Tooltip label="Restore tab">
              <button
                type="button"
                class="tab-item-action"
                aria-label="Restore tab"
                onClick={() => props.onRestore!()}
              >
                <Icon name="arrow-u-down-left" size={14} />
              </button>
            </Tooltip>
          )}
          {props.onClose && (
            <Tooltip label="Remove from Recent">
              <button
                type="button"
                class="tab-item-action tab-item-close"
                aria-label="Remove from Recent"
                onClick={() => props.onClose!()}
              >
                <Icon name="x" size={14} />
              </button>
            </Tooltip>
          )}
        </div>
      </Show>
    </div>
  );
}
