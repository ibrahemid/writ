import { createSignal, Show, For, createMemo, onCleanup } from "solid-js";
import { spellingStore, entryKey } from "../../stores/global/spelling";
import type { SpellingEntry } from "../../editor/spelling";
import "./SpellingPreview.css";

// Non-modal overlay listing every flagged range as `wrong → fix`, with
// per-row selection, check-all, and Apply. Snapshots the live entries on open;
// applying and ignoring both reconcile against the editor's current ranges.

// Singleton state — Writ is single-window.
const [isOpen, setIsOpen] = createSignal(false);
const [rows, setRows] = createSignal<SpellingEntry[]>([]);
const [selected, setSelected] = createSignal<Set<string>>(new Set());

export function openSpellingPreview() {
  const entries = spellingStore.entries();
  setRows(entries);
  // Every fixable row starts checked.
  setSelected(new Set(entries.filter((e) => e.suggestions.length > 0).map(entryKey)));
  setIsOpen(true);
}

export function closeSpellingPreview() {
  setIsOpen(false);
  setRows([]);
  setSelected(new Set<string>());
}

function isFixable(entry: SpellingEntry): boolean {
  return entry.confident && entry.suggestions.length > 0;
}

export default function SpellingPreview() {
  let panelRef: HTMLDivElement | undefined;

  const fixableRows = createMemo(() => rows().filter(isFixable));
  const allChecked = createMemo(() => {
    const fixable = fixableRows();
    return fixable.length > 0 && fixable.every((e) => selected().has(entryKey(e)));
  });

  function toggleRow(entry: SpellingEntry, on: boolean) {
    const next = new Set(selected());
    if (on) next.add(entryKey(entry));
    else next.delete(entryKey(entry));
    setSelected(next);
  }

  function toggleAll(on: boolean) {
    setSelected(on ? new Set(fixableRows().map(entryKey)) : new Set<string>());
  }

  function apply() {
    const chosen = rows().filter((e) => isFixable(e) && selected().has(entryKey(e)));
    spellingStore.applyEntries(chosen);
    closeSpellingPreview();
  }

  async function ignore(entry: SpellingEntry) {
    await spellingStore.ignoreWord(entry.word);
    setRows((prev) => prev.filter((e) => e.word !== entry.word));
    const next = new Set(selected());
    for (const e of rows()) {
      if (e.word === entry.word) next.delete(entryKey(e));
    }
    setSelected(next);
  }

  function onKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      closeSpellingPreview();
    }
  }

  onCleanup(() => closeSpellingPreview());

  return (
    <Show when={isOpen()}>
      <div
        ref={panelRef}
        class="spelling-preview"
        role="dialog"
        aria-label="Spelling preview"
        onKeyDown={onKeyDown}
      >
        <div class="spelling-preview-head">
          <label class="spelling-preview-all">
            <input
              type="checkbox"
              checked={allChecked()}
              onChange={(e) => toggleAll(e.currentTarget.checked)}
              aria-label="Select all fixes"
            />
            <span>{fixableRows().length} to fix</span>
          </label>
          <div class="spelling-preview-actions">
            <button
              type="button"
              class="spelling-preview-btn spelling-preview-apply"
              disabled={selected().size === 0}
              onClick={apply}
            >
              Apply
            </button>
            <button
              type="button"
              class="spelling-preview-btn"
              onClick={closeSpellingPreview}
              aria-label="Close spelling preview"
            >
              Close
            </button>
          </div>
        </div>

        <Show
          when={rows().length > 0}
          fallback={<div class="spelling-preview-empty">Nothing to fix</div>}
        >
          <ul class="spelling-preview-list">
            <For each={rows()}>
              {(entry) => (
                <li class="spelling-preview-row" onClick={() => spellingStore.reveal(entry)}>
                  <input
                    type="checkbox"
                    class="spelling-preview-check"
                    checked={selected().has(entryKey(entry))}
                    disabled={!isFixable(entry)}
                    onClick={(e) => e.stopPropagation()}
                    onChange={(e) => toggleRow(entry, e.currentTarget.checked)}
                    aria-label={`Fix ${entry.word}`}
                  />
                  <span class="spelling-preview-word">{entry.word}</span>
                  <Show when={entry.suggestions.length > 0}>
                    <span class="spelling-preview-arrow" aria-hidden="true">→</span>
                    <span class="spelling-preview-fix">
                      {entry.suggestions[0] === "" ? "(remove)" : entry.suggestions[0]}
                    </span>
                  </Show>
                  <Show when={entry.kind === "Spelling"}>
                    <button
                      type="button"
                      class="spelling-preview-ignore"
                      onClick={(e) => {
                        e.stopPropagation();
                        void ignore(entry);
                      }}
                    >
                      Ignore
                    </button>
                  </Show>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </div>
    </Show>
  );
}
