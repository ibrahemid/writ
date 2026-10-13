import {
  createContext,
  createEffect,
  createUniqueId,
  Show,
  useContext,
  type JSX,
} from "solid-js";

import type { SettingsSection } from "../../settings";

export interface SearchContextValue {
  rowVisible: (id: string) => boolean;
  sectionVisible: (section: SettingsSection) => boolean;
  highlighted: (id: string) => boolean;
}

export const SearchContext = createContext<SearchContextValue>();

export function useSearch(): SearchContextValue {
  const ctx = useContext(SearchContext);
  if (!ctx) throw new Error("SettingsRow used outside SettingsModal");
  return ctx;
}

interface ToggleSwitchProps {
  setting: string;
  label: string;
  checked: boolean;
  disabled?: boolean;
  onChange: () => void;
}

/** 32 x 18 track, 14px knob. A button, so Space and Enter activate it natively. */
export function ToggleSwitch(props: ToggleSwitchProps) {
  return (
    <button
      type="button"
      class="settings-switch"
      classList={{ "settings-switch-on": props.checked }}
      data-setting={props.setting}
      role="switch"
      aria-checked={props.checked}
      aria-label={props.label}
      disabled={props.disabled}
      onClick={props.onChange}
    >
      <span class="settings-switch-knob" />
    </button>
  );
}

interface SettingsRowProps {
  id: string;
  label: string;
  labelFor?: string;
  /** What the row does, in the label's slot and a neutral tone. */
  description?: string;
  /** What the row warns about, in the same slot. */
  caution?: string;
  /** A control that belongs with the label rather than with the row's own
   * control, such as the link to a provider's key page. */
  labelAside?: JSX.Element;
  align?: "start";
  children: JSX.Element;
}

/** What a row's description can be attached to, in the order a row holds one. */
const ROW_CONTROL = '[role="switch"], [role="radiogroup"], input, select, textarea, button';

export function SettingsRow(props: SettingsRowProps) {
  const search = useSearch();
  const descriptionId = createUniqueId();
  const cautionId = createUniqueId();
  let rowRef: HTMLDivElement | undefined;

  const describedBy = () =>
    [props.description ? descriptionId : "", props.caution ? cautionId : ""]
      .filter(Boolean)
      .join(" ");

  // The control is `children`, so the row cannot hand it a prop. It finds the
  // control inside its own element instead: the description belongs on the
  // control as a description, not inside the label, where it became part of the
  // control's accessible name.
  createEffect(() => {
    const ids = describedBy();
    if (!rowRef) return;
    // Not simply the first match: `labelAside` puts a control inside the label
    // column, ahead of the row's own in document order.
    const control = props.labelFor
      ? rowRef.querySelector<HTMLElement>(`[id="${props.labelFor}"]`)
      : (Array.from(rowRef.querySelectorAll<HTMLElement>(ROW_CONTROL)).find(
          (el) => !el.closest(".settings-row-label"),
        ) ?? null);
    if (!control) return;
    if (ids) control.setAttribute("aria-describedby", ids);
    else control.removeAttribute("aria-describedby");
  });

  return (
    <Show when={search.rowVisible(props.id)}>
      <div
        ref={rowRef}
        class="settings-row"
        classList={{ "settings-row-highlight": search.highlighted(props.id) }}
        data-setting-id={props.id}
        data-align={props.align}
      >
        <span class="settings-row-label">
          <Show
            when={props.labelFor}
            fallback={<span class="settings-row-label-text">{props.label}</span>}
          >
            <label class="settings-row-label-text" for={props.labelFor}>
              {props.label}
            </label>
          </Show>
          {props.labelAside}
          <Show when={props.description}>
            <span id={descriptionId} class="settings-row-description">
              {props.description}
            </span>
          </Show>
          <Show when={props.caution}>
            <span id={cautionId} class="settings-row-caution">
              {props.caution}
            </span>
          </Show>
        </span>
        {props.children}
      </div>
    </Show>
  );
}
