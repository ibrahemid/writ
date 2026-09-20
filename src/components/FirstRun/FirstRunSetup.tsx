import { For, onMount } from "solid-js";
import { firstRunStore } from "../../stores/global/first-run";
import type { FileExtension } from "../../types/config";
import Button from "../Button/Button";
import { logFailure } from "../../lib/log";
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

  // The answer is reachable from the keyboard the moment the screen appears,
  // and the reader is put on the option that is already chosen.
  onMount(() => {
    const index = FORMATS.findIndex((option) => option.value === firstRunStore.format());
    options[index === -1 ? 0 : index]?.focus();
  });

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
      case "Enter":
        choose(at);
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  return (
    <div class="first-run-setup">
      <div class="first-run-setup-panel">
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
          onClick={() =>
            void firstRunStore
              .continueSetup()
              .catch(() => logFailure("the first launch could not be finished"))
          }
        >
          Continue
        </Button>
      </div>
    </div>
  );
}
