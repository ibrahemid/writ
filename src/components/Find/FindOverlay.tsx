import { Show, For, createMemo, createEffect, onCleanup } from "solid-js";
import { findStore, type FindController } from "../../stores/global/find-store";
import { useCommand } from "../../commands/registry";
import { useEffectiveBinding } from "../../commands/keybindings";
import { formatKeybinding } from "../../lib/keybinding-format";
import Button from "../Button/Button";
import Tooltip from "../Tooltip/Tooltip";
import "./FindOverlay.css";

const MAX_TICK = 200;

/** The option toggles share one class; the accent belongs to the on state. */
const toggleClass = (on: boolean): string => `find-toggle${on ? " is-on" : ""}`;

interface Props {
  store?: FindController;
}

export default function FindOverlay(props: Props) {
  const find = props.store ?? findStore;
  let queryInput: HTMLInputElement | undefined;

  createEffect(() => {
    find.focusNonce();
    if (!find.isOpen()) return;
    const el = queryInput;
    if (!el) return;
    requestAnimationFrame(() => {
      el.focus();
      el.select();
    });
  });

  const hasQuery = createMemo(() => find.queryText().trim().length > 0);

  const countLabel = createMemo(() => {
    const { current, total, capped } = find.matches();
    if (!hasQuery()) return "";
    if (total === 0) return "No results";
    const suffix = capped ? "+" : "";
    if (current > 0) return `${current} of ${total}${suffix}`;
    return `${total}${suffix} match${total === 1 ? "" : "es"}`;
  });

  // The row's own control for the command the app registers as Replace, so it
  // carries that name and that key rather than a second name for one action.
  const replaceTitle = createMemo(() => {
    const key = formatKeybinding(
      useEffectiveBinding("editor.replace", useCommand("editor.replace")?.keybinding),
    );
    return key ? `Replace (${key})` : "Replace";
  });

  const noResults = createMemo(() => hasQuery() && find.matches().total === 0);
  const currentTick = createMemo(() => find.matches().current);

  function onQueryKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      find.close();
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (e.shiftKey) find.previous();
      else find.next();
    }
  }

  function onReplaceKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      find.close();
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (e.shiftKey) find.replaceAll();
      else find.replaceCurrent();
    }
  }

  onCleanup(() => find.close());

  return (
    <Show when={find.isOpen()}>
      <div class="find-tickmap" aria-hidden="true">
        <For each={find.ticks().slice(0, MAX_TICK)}>
          {(tick, i) => (
            <span
              class="find-tick"
              classList={{ "is-current": i() + 1 === currentTick() }}
              style={{ top: `${(tick.fraction * 100).toFixed(3)}%` }}
            />
          )}
        </For>
      </div>

      <div class="find-overlay" role="search" aria-label="Find in document">
        <div class="find-row">
          <input
            ref={(el) => (queryInput = el)}
            class="find-input"
            type="text"
            data-writ-focus-silent
            spellcheck={false}
            autocomplete="off"
            placeholder="Find"
            aria-label="Find"
            value={find.queryText()}
            onInput={(e) => find.setQueryText(e.currentTarget.value)}
            onKeyDown={onQueryKeyDown}
          />
          <span
            class="find-count"
            classList={{ "is-empty": noResults() }}
            aria-live="polite"
          >
            {countLabel()}
          </span>

          <div class="find-toggles">
            <Tooltip label="Match case">
              <Button
                variant="ghost"
                class={toggleClass(find.caseSensitive())}
                pressed={find.caseSensitive()}
                aria-label="Match case"
                onClick={() => find.toggleCaseSensitive()}
              >
                Aa
              </Button>
            </Tooltip>
            <Tooltip label="Whole word">
              <Button
                variant="ghost"
                class={toggleClass(find.wholeWord())}
                pressed={find.wholeWord()}
                aria-label="Whole word"
                onClick={() => find.toggleWholeWord()}
              >
                <span class="find-toggle-word">ab</span>
              </Button>
            </Tooltip>
            <Tooltip label="Regular expression">
              <Button
                variant="ghost"
                class={toggleClass(find.regexp())}
                pressed={find.regexp()}
                aria-label="Regular expression"
                onClick={() => find.toggleRegexp()}
              >
                .*
              </Button>
            </Tooltip>
          </div>

          <div class="find-nav">
            <Tooltip label="Previous match (Shift+Enter)">
              <Button
                variant="ghost"
                class="find-icon-btn"
                icon="caret-up"
                iconSize={12}
                aria-label="Previous match"
                disabled={!hasQuery()}
                onClick={() => find.previous()}
              />
            </Tooltip>
            <Tooltip label="Next match (Enter)">
              <Button
                variant="ghost"
                class="find-icon-btn"
                icon="caret-down"
                iconSize={12}
                aria-label="Next match"
                disabled={!hasQuery()}
                onClick={() => find.next()}
              />
            </Tooltip>
          </div>

          <Show when={find.canReplace()}>
            <Tooltip label={replaceTitle()}>
              <Button
                variant="ghost"
                class={`find-icon-btn find-replace-toggle${find.replaceOpen() ? " is-on" : ""}`}
                icon="caret-right"
                iconSize={12}
                aria-label="Replace"
                aria-expanded={find.replaceOpen()}
                onClick={() => find.toggleReplace()}
              />
            </Tooltip>
          </Show>
          <Tooltip label="Close (Esc)">
            <Button
              variant="ghost"
              class="find-icon-btn"
              icon="x"
              iconSize={14}
              aria-label="Close"
              onClick={() => find.close()}
            />
          </Tooltip>
        </div>

        <Show when={find.replaceOpen() && find.canReplace()}>
          <div class="find-row find-row-replace">
            <input
              class="find-input"
              type="text"
              data-writ-focus-silent
              spellcheck={false}
              autocomplete="off"
              placeholder="Replace"
              aria-label="Replace with"
              value={find.replaceText()}
              onInput={(e) => find.setReplaceText(e.currentTarget.value)}
              onKeyDown={onReplaceKeyDown}
            />
            <Tooltip label="Replace (Enter)">
              <Button
                class="find-text-btn"
                disabled={!hasQuery()}
                onClick={() => find.replaceCurrent()}
              >
                Replace
              </Button>
            </Tooltip>
            <Tooltip label="Replace all (Shift+Enter)">
              <Button
                class="find-text-btn"
                aria-label="Replace all"
                disabled={!hasQuery()}
                onClick={() => find.replaceAll()}
              >
                All
              </Button>
            </Tooltip>
          </div>
        </Show>
      </div>
    </Show>
  );
}
