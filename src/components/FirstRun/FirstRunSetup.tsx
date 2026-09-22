import { For, Match, Show, Switch, onCleanup, onMount } from "solid-js";
import { firstRunStore } from "../../stores/global/first-run";
import type { FileExtension } from "../../types/config";
import Button from "../Button/Button";
import { installFocusTrap } from "../../lib/focus-trap";
import { APPS } from "../../lib/apps";
import "./FirstRunSetup.css";

interface FormatOption {
  value: FileExtension;
  label: string;
  detail: string;
}

const FORMATS: readonly FormatOption[] = [
  { value: "txt", label: "Plain text (.txt)", detail: "No markup. Opens in any editor." },
  {
    value: "md",
    label: "Markdown (.md)",
    detail: "Headings, lists and links, with a rendered view.",
  },
];

const HEADING_ID = "first-run-format-heading";
const APPS_HEADING_ID = "first-run-apps-heading";

/**
 * The questions a first launch asks, one step at a time, on the window it is
 * about to fill.
 *
 * It stands on the editor's own background rather than over a scrim: there is
 * no note behind it yet, and a dimmed window would be dimming nothing. The
 * screen leaves on the last Continue, which is also the moment anything is
 * written.
 */
export default function FirstRunSetup() {
  return (
    <div class="first-run-setup">
      <Switch>
        <Match when={firstRunStore.step() === "format"}>
          <FormatStep />
        </Match>
        <Match when={firstRunStore.step() === "apps"}>
          <AppsStep />
        </Match>
      </Switch>
    </div>
  );
}

/** Enter answers a step from wherever the reader is on it, because each step
 * asks one question and Continue is its answer. A button's own Enter is left
 * to the button, so a switch toggles and the answer is not taken twice. */
function answerOnEnter(event: KeyboardEvent, submit: () => void): void {
  if (event.key !== "Enter") return;
  if (event.target instanceof HTMLButtonElement) return;
  event.preventDefault();
  submit();
}

function FormatStep() {
  const options: HTMLDivElement[] = [];
  let panel: HTMLDivElement | undefined;

  // The answer is reachable from the keyboard the moment the screen appears,
  // and the reader is put on the option that is already chosen.
  //
  // The trap goes on first: the screen is the only thing on the window that
  // can be answered, so Tab stays on the two options and Continue rather than
  // walking into chrome that does nothing yet. It also inerts the app behind
  // the screen, and the chord handler reads the same step this screen does, so
  // nothing opens underneath it.
  onMount(() => {
    if (panel) onCleanup(installFocusTrap(panel));
    const index = FORMATS.findIndex((option) => option.value === firstRunStore.format());
    options[index === -1 ? 0 : index]?.focus();
  });

  function submit(): void {
    firstRunStore.continueFormat();
  }

  function choose(index: number): void {
    const option = FORMATS[index];
    if (!option) return;
    firstRunStore.setFormat(option.value);
    options[index]?.focus();
  }

  function onKeyDown(event: KeyboardEvent): void {
    const current = FORMATS.findIndex((option) => option.value === firstRunStore.format());
    const at = current === -1 ? 0 : current;
    switch (event.key) {
      case "ArrowRight":
      case "ArrowDown":
        choose((at + 1) % FORMATS.length);
        break;
      case "ArrowLeft":
      case "ArrowUp":
        choose((at - 1 + FORMATS.length) % FORMATS.length);
        break;
      case " ":
        choose(at);
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  return (
    <div
        class="first-run-setup-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby={HEADING_ID}
        ref={panel}
        onKeyDown={(event) => answerOnEnter(event, submit)}
      >
        <h1 class="first-run-setup-heading" id={HEADING_ID}>
          Default format
        </h1>
        <div
          class="first-run-formats"
          role="radiogroup"
          aria-labelledby={HEADING_ID}
          onKeyDown={onKeyDown}
        >
          <For each={FORMATS}>
            {(option, index) => {
              const selected = () => firstRunStore.format() === option.value;
              return (
                <div
                  ref={(el) => (options[index()] = el)}
                  class="first-run-format"
                  classList={{ "is-selected": selected() }}
                  role="radio"
                  aria-checked={selected()}
                  tabindex={selected() ? 0 : -1}
                  data-format={option.value}
                  onClick={() => choose(index())}
                >
                  <span class="first-run-format-label">{option.label}</span>
                  <span class="first-run-format-detail">{option.detail}</span>
                </div>
              );
            }}
          </For>
        </div>
        <Button
          variant="primary"
          class="first-run-setup-continue"
          onClick={submit}
        >
          Continue
        </Button>
    </div>
  );
}

/**
 * The six apps, each off until switched on (ADR-042 section 5). A first
 * launch reaches this after the format; Settings, Apps opens it again, and
 * there Continue writes the switches and opens nothing.
 */
function AppsStep() {
  let panel: HTMLDivElement | undefined;
  const switches: HTMLButtonElement[] = [];

  // Opened from Settings, Escape leaves the switches as they were; a first
  // launch has nothing to go back to, so there it does nothing.
  onMount(() => {
    if (panel) {
      onCleanup(
        installFocusTrap(panel, {
          onEscape: () => {
            if (firstRunStore.revisiting()) firstRunStore.cancelApps();
          },
        }),
      );
    }
    switches[0]?.focus();
  });

  function submit(): void {
    void firstRunStore.continueApps();
  }

  return (
    <div
      class="first-run-setup-panel"
      role="dialog"
      aria-modal="true"
      aria-labelledby={APPS_HEADING_ID}
      ref={panel}
      onKeyDown={(event) => answerOnEnter(event, submit)}
    >
      <h1 class="first-run-setup-heading" id={APPS_HEADING_ID}>
        Apps
      </h1>
      <ul class="first-run-apps" aria-labelledby={APPS_HEADING_ID}>
        <For each={APPS}>
          {(app, index) => {
            const on = () => firstRunStore.isAppChosen(app.id);
            const labelId = `first-run-app-${app.id}`;
            const detailId = `first-run-app-${app.id}-detail`;
            return (
              <li class="first-run-app" data-app={app.id} onClick={() => firstRunStore.toggleApp(app.id)}>
                <span class="first-run-app-text">
                  <span class="first-run-app-label" id={labelId}>
                    {app.label}
                  </span>
                  <span class="first-run-app-detail" id={detailId}>
                    {app.detail}
                  </span>
                </span>
                <button
                  ref={(el) => (switches[index()] = el)}
                  type="button"
                  class="first-run-app-switch"
                  classList={{ "is-on": on() }}
                  role="switch"
                  aria-checked={on()}
                  aria-labelledby={labelId}
                  aria-describedby={detailId}
                  onClick={(event) => {
                    event.stopPropagation();
                    firstRunStore.toggleApp(app.id);
                  }}
                >
                  <span class="first-run-app-knob" />
                </button>
              </li>
            );
          }}
        </For>
      </ul>
      <div class="first-run-setup-actions">
        <Button
          variant="primary"
          class="first-run-setup-continue"
          disabled={firstRunStore.busy()}
          aria-busy={firstRunStore.busy()}
          onClick={submit}
        >
          Continue
        </Button>
        <Show when={firstRunStore.revisiting()}>
          <Button data-action="cancel-apps" onClick={() => firstRunStore.cancelApps()}>
            Cancel
          </Button>
        </Show>
      </div>
    </div>
  );
}
