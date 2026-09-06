import { Show, createUniqueId, type JSX } from "solid-js";
import Icon from "../Icon/Icon";
import { useWindow } from "../WindowProvider/WindowProvider";
import type { RightPanelSection } from "../../stores/window/right-panel-store";

interface Props {
  section: RightPanelSection;
  /** The heading, and the name the section is announced under. */
  heading: string;
  children: JSX.Element;
}

/**
 * One headed, foldable section of the panel.
 *
 * The heading is the control: a section is announced as a heading for a reader
 * walking the document and as a disclosure for one operating it, without a
 * second element beside the title doing the same job.
 *
 * A section with nothing in it renders none of this — the panel decides that,
 * so a heading never stands over an empty list.
 */
export default function PanelSection(props: Props) {
  const win = useWindow();
  const headingId = createUniqueId();
  const open = () => !win.rightPanel.isCollapsed(props.section);

  return (
    <section class="right-panel-section" aria-labelledby={headingId}>
      <h2 class="right-panel-section-title" id={headingId}>
        <button
          type="button"
          class="right-panel-section-toggle"
          aria-expanded={open()}
          onClick={() => win.rightPanel.toggleSection(props.section)}
        >
          <Icon name={open() ? "caret-down" : "caret-right"} size={12} />
          {props.heading}
        </button>
      </h2>
      <Show when={open()}>{props.children}</Show>
    </section>
  );
}
