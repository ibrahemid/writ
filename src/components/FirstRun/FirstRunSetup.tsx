import { For, onCleanup, onMount } from "solid-js";
import { firstRunStore } from "../../stores/global/first-run";
import type { FileExtension } from "../../types/config";
import Button from "../Button/Button";
import { installFocusTrap } from "../../lib/focus-trap";
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

/**
 * The one question a first launch asks, on the window it is about to fill.
 *
 * It stands on the editor's own background rather than over a scrim: there is
 * no note behind it yet, and a dimmed window would be dimming nothing. The
 * screen leaves on Continue, which is also the moment anything is written.
 */
export default function FirstRunSetup() {
  const options: HTMLDivElement[] = [];
  let panel: HTMLDivElement | undefined;
  let confirm: HTMLButtonElement | undefined;

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
    void firstRunStore.continueSetup();
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

  // Enter answers the screen from wherever the reader is on it, because the
  // screen asks one question and Continue is its only answer. The button's own
  // Enter is left to the button, so the answer is not taken twice.
  function onPanelKeyDown(event: KeyboardEvent): void {
    if (event.key !== "Enter") return;
    if (event.target === confirm) return;
    event.preventDefault();
    submit();
  }

  return (
    <div class="first-run-setup">
      <div
        class="first-run-setup-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby={HEADING_ID}
        ref={panel}
        onKeyDown={onPanelKeyDown}
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
          ref={(el) => (confirm = el)}
          variant="primary"
          class="first-run-setup-continue"
          disabled={firstRunStore.busy()}
          aria-busy={firstRunStore.busy()}
          onClick={submit}
        >
          Continue
        </Button>
      </div>
    </div>
  );
}
