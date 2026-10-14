import { For, Show, splitProps } from "solid-js";
import { configStore } from "../../stores/global/config";
import { showToast } from "../../stores/global/notifications";
import { SECTION_LABELS, type SettingsSection } from "../../settings";
import Tooltip from "../Tooltip/Tooltip";
import { useSearch } from "./SettingsRow";

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

export function parseIntSafe(raw: string, fallback: number): number {
  const n = parseInt(raw, 10);
  return Number.isFinite(n) ? n : fallback;
}

export function parseFloatSafe(raw: string, fallback: number): number {
  const n = parseFloat(raw);
  return Number.isFinite(n) ? n : fallback;
}

type WritConfigValue = ReturnType<typeof configStore.config>;

export async function patchConfig(patch: (prev: WritConfigValue) => WritConfigValue) {
  try {
    await configStore.save(patch(configStore.config()));
  } catch {
    showToast("Could not save your settings", "error");
  }
}

interface SegmentedProps<T extends string> {
  setting: string;
  label: string;
  options: { id: T; label: string }[];
  value: T;
  onChange: (id: T) => void;
}

/**
 * A radiogroup, so the arrow keys move the choice and Tab leaves the group.
 * Only the checked cell is tabbable; that is the roving-tabindex the pattern
 * asks for.
 */
export function Segmented<T extends string>(props: SegmentedProps<T>) {
  let groupRef: HTMLDivElement | undefined;

  function move(delta: number) {
    const ids = props.options.map((o) => o.id);
    const current = ids.indexOf(props.value);
    const next = ids[(current + delta + ids.length) % ids.length];
    props.onChange(next);
    requestAnimationFrame(() =>
      groupRef?.querySelector<HTMLButtonElement>(`[data-option="${next}"]`)?.focus(),
    );
  }

  function onKeyDown(event: KeyboardEvent) {
    switch (event.key) {
      case "ArrowRight":
      case "ArrowDown":
        event.preventDefault();
        move(1);
        break;
      case "ArrowLeft":
      case "ArrowUp":
        event.preventDefault();
        move(-1);
        break;
    }
  }

  return (
    <div
      ref={groupRef}
      class="settings-seg"
      role="radiogroup"
      aria-label={props.label}
      data-setting={props.setting}
      onKeyDown={onKeyDown}
    >
      <For each={props.options}>
        {(option) => (
          <button
            type="button"
            class="settings-seg-option"
            role="radio"
            data-option={option.id}
            aria-checked={props.value === option.id}
            tabindex={props.value === option.id ? 0 : -1}
            onClick={() => props.onChange(option.id)}
          >
            {option.label}
          </button>
        )}
      </For>
    </div>
  );
}

export function SectionLabel(props: { section: SettingsSection }) {
  const search = useSearch();
  return (
    <Show when={search.sectionVisible(props.section)}>
      <h3 class="settings-section-label">{SECTION_LABELS[props.section]}</h3>
    </Show>
  );
}

interface PathTextProps {
  class: string;
  /** The full path, for the tooltip; defaults to the text shown. */
  full?: string;
  text: string;
  [data: `data-${string}`]: boolean | string | undefined;
}

/**
 * A path or command cut from its start when it does not fit. The box runs
 * right to left so the ellipsis lands at the start, and the text inside is
 * isolated left to right, so `~/Writ` and a leading slash keep their place and
 * an Arabic folder name reads in its own order within the path.
 */
export function PathText(props: PathTextProps) {
  const [local, data] = splitProps(props, ["class", "full", "text"]);
  return (
    <Tooltip label={local.full ?? local.text}>
      <span class={local.class} {...data}>
        <bdi dir="ltr">{local.text}</bdi>
      </span>
    </Tooltip>
  );
}
