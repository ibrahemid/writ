import {
  createContext,
  createEffect,
  createUniqueId,
  Show,
  useContext,
  type JSX,
} from "solid-js";

import { settingDescription, settingTitle, type SettingsSection } from "../../settings";

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
  id?: string;
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
      id={props.id}
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
  labelFor?: string;
  /** What the row does, in the label's slot and a neutral tone. The index
   * holds every row's sentence; a row passes its own only when the sentence
   * depends on state. */
  description?: string;
  /** Drawn under the description: the chords a row names, or a sample of
   * what the row changes. */
  detail?: JSX.Element;
  /** What the row warns about, in the same slot. */
  caution?: string;
  /** A control that belongs with the label rather than with the row's own
   * control, such as the link to a provider's key page. */
  labelAside?: JSX.Element;
  /** What follows the description in the label column, such as where an app
   * lives and the control that opens it. Kept out of the row's control
   * column, so the description stays on the row's own control. */
  footer?: JSX.Element;
  align?: "start";
  /** `full`: a list the section heading names, its label for screen readers
   * alone. `stacked`: the label above a control that takes the row's width. */
  layout?: "full" | "stacked";
  children: JSX.Element;
}

/** What a row's description can be attached to, in the order a row holds one. */
const ROW_CONTROL = '[role="switch"], [role="radiogroup"], input, select, textarea, button';

export function SettingsRow(props: SettingsRowProps) {
  const search = useSearch();
  const label = () => settingTitle(props.id);
  const description = () => props.description ?? settingDescription(props.id);
  const descriptionId = createUniqueId();
  const cautionId = createUniqueId();
  let rowRef: HTMLDivElement | undefined;

  const describedBy = () =>
    [description() ? descriptionId : "", props.caution ? cautionId : ""]
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
        data-layout={props.layout}
      >
        <span class="settings-row-label">
          <Show
            when={props.labelFor}
            fallback={<span class="settings-row-label-text">{label()}</span>}
          >
            <label class="settings-row-label-text" for={props.labelFor}>
              {label()}
            </label>
          </Show>
          {props.labelAside}
          <Show when={description()}>
            <span id={descriptionId} class="settings-row-description">
              {description()}
            </span>
          </Show>
          <Show when={props.detail}>
            <span class="settings-row-detail">{props.detail}</span>
          </Show>
          <Show when={props.caution}>
            <span id={cautionId} class="settings-row-caution">
              {props.caution}
            </span>
          </Show>
          {props.footer}
        </span>
        {props.children}
      </div>
    </Show>
  );
}

interface SwitchRowProps {
  id: string;
  setting: string;
  checked: boolean;
  disabled?: boolean;
  description?: string;
  caution?: string;
  detail?: JSX.Element;
  footer?: JSX.Element;
  aside?: JSX.Element;
  onChange: () => void;
}

export function SwitchRow(props: SwitchRowProps) {
  const switchId = createUniqueId();
  const toggle = () => (
    <ToggleSwitch
      id={switchId}
      setting={props.setting}
      label={settingTitle(props.id)}
      checked={props.checked}
      disabled={props.disabled}
      onChange={props.onChange}
    />
  );
  return (
    <SettingsRow
      id={props.id}
      labelFor={switchId}
      description={props.description}
      caution={props.caution}
      detail={props.detail}
      footer={props.footer}
    >
      {props.aside ? (
        <span class="settings-switch-row-controls">
          {toggle()}
          {props.aside}
        </span>
      ) : (
        toggle()
      )}
    </SettingsRow>
  );
}
