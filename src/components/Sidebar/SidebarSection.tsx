import { Show, createUniqueId, type JSX } from "solid-js";
import Icon from "../Icon/Icon";
import { configStore } from "../../stores/global/config";
import type { SidebarSectionId } from "../../types/config";

interface Props {
  id: SidebarSectionId;
  /** The heading, and the name the section is announced under. */
  heading: string;
  /** How many rows the section holds; stays beside the heading when folded. */
  count?: number;
  /** A control that belongs to the heading row, after the toggle: closing the folder, say. */
  action?: JSX.Element;
  class?: string;
  /** Set for a section a command scrolls to and focuses. */
  focusable?: boolean;
  ref?: (el: HTMLElement) => void;
  children: JSX.Element;
}

/**
 * One headed, foldable section of the sidebar, the same control the panel
 * beside the note uses: the heading is a disclosure, folded state lasts across
 * launches, and a section switched off in Settings renders nothing.
 *
 * A section with nothing in it renders none of this. The section decides
 * that, so a heading never stands over an empty list.
 */
export default function SidebarSection(props: Props) {
  const headingId = createUniqueId();
  const open = () => !configStore.isSidebarSectionCollapsed(props.id);

  return (
    <Show when={!configStore.isSidebarSectionHidden(props.id)}>
      <section
        class={`sidebar-section ${props.class ?? ""}`.trim()}
        aria-labelledby={headingId}
        tabindex={props.focusable ? -1 : undefined}
        ref={props.ref}
      >
        <div class="sidebar-section-head">
          <h2 class="sidebar-section-heading" id={headingId}>
            <button
              type="button"
              class="sidebar-section-toggle"
              aria-expanded={open()}
              onClick={() => configStore.setSidebarSectionCollapsed(props.id, open())}
            >
              <span class="sidebar-section-caret" aria-hidden="true">
                <Icon name={open() ? "caret-down" : "caret-right"} size={12} />
              </span>
              <span class="sidebar-section-name">{props.heading}</span>
              <Show when={props.count !== undefined}>
                <span class="sidebar-section-count">{props.count}</span>
              </Show>
            </button>
          </h2>
          <Show when={props.action}>
            <div class="sidebar-section-action-slot">{props.action}</div>
          </Show>
        </div>
        <Show when={open()}>{props.children}</Show>
      </section>
    </Show>
  );
}
