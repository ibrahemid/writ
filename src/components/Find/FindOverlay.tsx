import { Show, For, createMemo, createEffect, onCleanup } from "solid-js";
import { findStore, type FindController } from "../../stores/global/find-store";
import "./FindOverlay.css";

const MAX_TICK = 200;

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
            <button
              type="button"
              class="find-toggle"
              classList={{ "is-on": find.caseSensitive() }}
              aria-pressed={find.caseSensitive()}
              title="Match case"
              onClick={() => find.toggleCaseSensitive()}
            >
              Aa
            </button>
            <button
              type="button"
              class="find-toggle"
              classList={{ "is-on": find.wholeWord() }}
              aria-pressed={find.wholeWord()}
              title="Whole word"
              onClick={() => find.toggleWholeWord()}
            >
              <span class="find-toggle-word">ab</span>
            </button>
            <button
              type="button"
              class="find-toggle"
              classList={{ "is-on": find.regexp() }}
              aria-pressed={find.regexp()}
              title="Regular expression"
              onClick={() => find.toggleRegexp()}
            >
              .*
            </button>
          </div>

          <div class="find-nav">
            <button
              type="button"
              class="find-icon-btn"
              title="Previous match (Shift+Enter)"
              aria-label="Previous match"
              disabled={!hasQuery()}
              onClick={() => find.previous()}
            >
              <ChevronUp />
            </button>
            <button
              type="button"
              class="find-icon-btn"
              title="Next match (Enter)"
              aria-label="Next match"
              disabled={!hasQuery()}
              onClick={() => find.next()}
            >
              <ChevronDown />
            </button>
          </div>

          <Show when={find.canReplace()}>
            <button
              type="button"
              class="find-icon-btn find-replace-toggle"
              classList={{ "is-on": find.replaceOpen() }}
              title="Toggle replace"
              aria-label="Toggle replace"
              aria-expanded={find.replaceOpen()}
              onClick={() => find.toggleReplace()}
            >
              <ChevronRight />
            </button>
          </Show>
          <button
            type="button"
            class="find-icon-btn"
            title="Close (Esc)"
            aria-label="Close find"
            onClick={() => find.close()}
          >
            <CloseIcon />
          </button>
        </div>

        <Show when={find.replaceOpen() && find.canReplace()}>
          <div class="find-row find-row-replace">
            <input
              class="find-input"
              type="text"
              spellcheck={false}
              autocomplete="off"
              placeholder="Replace"
              aria-label="Replace"
              value={find.replaceText()}
              onInput={(e) => find.setReplaceText(e.currentTarget.value)}
              onKeyDown={onReplaceKeyDown}
            />
            <button
              type="button"
              class="find-text-btn"
              title="Replace (Enter)"
              disabled={!hasQuery()}
              onClick={() => find.replaceCurrent()}
            >
              Replace
            </button>
            <button
              type="button"
              class="find-text-btn"
              title="Replace all (Shift+Enter)"
              disabled={!hasQuery()}
              onClick={() => find.replaceAll()}
            >
              All
            </button>
          </div>
        </Show>
      </div>
    </Show>
  );
}

function ChevronUp() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <path d="M3 7.5L6 4.5L9 7.5" stroke="currentColor" stroke-width="1.4" fill="none" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
  );
}

function ChevronDown() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <path d="M3 4.5L6 7.5L9 4.5" stroke="currentColor" stroke-width="1.4" fill="none" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
  );
}

function ChevronRight() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <path d="M4.5 3L7.5 6L4.5 9" stroke="currentColor" stroke-width="1.4" fill="none" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
  );
}

function CloseIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <path d="M3 3L9 9M9 3L3 9" stroke="currentColor" stroke-width="1.4" fill="none" stroke-linecap="round" />
    </svg>
  );
}
