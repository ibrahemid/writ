import { For, Show, createMemo, createEffect, createSignal, onCleanup, untrack } from "solid-js";
import { installFocusTrap } from "../../lib/focus-trap";
import { useWindow } from "../WindowProvider/WindowProvider";
import Kbd from "../Kbd/Kbd";
import { composeSections, type ComposedSection } from "./compose";
import { parsePaletteQuery } from "./query";
import { providerModes, type PaletteResult, type ResultProvider } from "./types";
import "./Palette.css";

export interface PaletteProps {
  open: boolean;
  onClose: () => void;
  providers: readonly ResultProvider[];
  placeholder: string;
  label: string;
  inputLabel: string;
  // Extra line under the results: index truncation, grep caps, hints.
  notice?: () => string | null;
  // Fired once per open, before the first query runs.
  onOpen?: () => void;
}

type Buckets = Record<string, PaletteResult[] | undefined>;

let paletteSeq = 0;

export default function Palette(props: PaletteProps) {
  const win = useWindow();
  const domId = `palette-${++paletteSeq}`;
  const listId = `${domId}-listbox`;
  const optionId = (index: number) => `${domId}-option-${index}`;

  const [query, setQuery] = createSignal("");
  const [buckets, setBuckets] = createSignal<Buckets>({});
  const [selectedIndex, setSelectedIndex] = createSignal(0);
  let inputRef: HTMLInputElement | undefined;
  let listRef: HTMLDivElement | undefined;
  let paletteRef: HTMLDivElement | undefined;
  let generation = 0;

  const parsed = createMemo(() => parsePaletteQuery(query()));

  const activeProviders = createMemo(() =>
    props.providers.filter((p) => providerModes(p).includes(parsed().mode)),
  );

  const composed = createMemo(() => composeSections(activeProviders(), buckets()));
  const flat = createMemo(() => composed().flat);
  const selected = createMemo(() => Math.min(selectedIndex(), Math.max(0, flat().length - 1)));

  createEffect(() => {
    if (!props.open) {
      setQuery("");
      setBuckets({});
      setSelectedIndex(0);
      generation += 1;
      return;
    }
    untrack(() => props.onOpen?.());
    requestAnimationFrame(() => inputRef?.focus());
  });

  // One generation per keystroke. Buckets are cleared synchronously here, not
  // when a response lands, so a new query never renders the old query's rows.
  createEffect(() => {
    if (!props.open) return;
    const text = parsed().text;
    const providers = activeProviders();

    generation += 1;
    const gen = generation;
    const controller = new AbortController();
    const timers: ReturnType<typeof setTimeout>[] = [];
    setBuckets({});
    setSelectedIndex(0);

    const isCurrent = () => gen === generation && !controller.signal.aborted;

    function replace(providerId: string, results: PaletteResult[]) {
      if (!isCurrent()) return;
      setBuckets((prev) => ({ ...prev, [providerId]: results }));
    }

    function append(providerId: string, results: PaletteResult[]) {
      if (!isCurrent() || results.length === 0) return;
      setBuckets((prev) => ({
        ...prev,
        [providerId]: [...(prev[providerId] ?? []), ...results],
      }));
    }

    function run(provider: ResultProvider) {
      if (!isCurrent()) return;
      let produced: PaletteResult[] | Promise<PaletteResult[]>;
      try {
        produced = provider.query(text, controller.signal);
      } catch (error) {
        console.error(`palette provider "${provider.id}" failed`, error);
        return;
      }
      if (produced instanceof Promise) {
        produced.then(
          (results) => replace(provider.id, results),
          (error) => console.error(`palette provider "${provider.id}" failed`, error),
        );
      } else {
        replace(provider.id, produced);
      }
      if (provider.stream) {
        provider
          .stream(text, (results) => append(provider.id, results), controller.signal)
          .catch((error) =>
            console.error(`palette provider "${provider.id}" stream failed`, error),
          );
      }
    }

    for (const provider of providers) {
      const delay = provider.debounceMs ?? 0;
      if (delay <= 0) {
        run(provider);
      } else {
        timers.push(setTimeout(() => run(provider), delay));
      }
    }

    onCleanup(() => {
      controller.abort();
      for (const timer of timers) clearTimeout(timer);
    });
  });

  createEffect(() => {
    if (!props.open || !paletteRef) return;
    const teardown = installFocusTrap(paletteRef, {
      onEscape: () => props.onClose(),
      fallbackRestore: () => {
        win.editor.focusEditor();
        return null;
      },
    });
    onCleanup(teardown);
  });

  function handleSelect(result: PaletteResult) {
    result.execute();
    props.onClose();
  }

  function scrollSelectedIntoView() {
    requestAnimationFrame(() => {
      if (!listRef) return;
      listRef.querySelector<HTMLElement>(".palette-item.is-selected")?.scrollIntoView({
        block: "nearest",
      });
    });
  }

  function handleKeyDown(e: KeyboardEvent) {
    const list = flat();
    if (e.key === "Escape") {
      props.onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex(Math.min(selected() + 1, list.length - 1));
      scrollSelectedIntoView();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex(Math.max(selected() - 1, 0));
      scrollSelectedIntoView();
    } else if (e.key === "Enter") {
      e.preventDefault();
      const result = list[selected()];
      if (result) handleSelect(result);
    }
  }

  function indexOf(result: PaletteResult): number {
    return flat().indexOf(result);
  }

  const countLabel = createMemo(() => {
    const n = flat().length;
    if (n === 0) return "No results";
    return `${n} result${n === 1 ? "" : "s"}`;
  });

  return (
    <Show when={props.open}>
      <div class="palette-overlay" onClick={() => props.onClose()}>
        <div
          ref={paletteRef}
          class="palette"
          onClick={(e) => e.stopPropagation()}
          role="dialog"
          aria-modal="true"
          aria-label={props.label}
        >
          <input
            ref={inputRef}
            type="text"
            class="palette-input"
            placeholder={props.placeholder}
            value={query()}
            onInput={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={handleKeyDown}
            aria-label={props.inputLabel}
            role="combobox"
            aria-expanded={flat().length > 0}
            aria-controls={listId}
            aria-autocomplete="list"
            aria-activedescendant={flat().length > 0 ? optionId(selected()) : undefined}
          />
          <Show
            when={flat().length > 0}
            fallback={
              <Show when={query().trim().length > 0}>
                <div class="palette-empty">
                  <div class="palette-empty-title">Nothing matches "{query()}"</div>
                  <div class="palette-empty-hint">
                    Try a different word, or press Esc to dismiss.
                  </div>
                </div>
              </Show>
            }
          >
            <div class="palette-results" ref={listRef} id={listId} role="listbox" aria-label={props.label}>
              <For each={composed().sections}>
                {(section) => (
                  <div
                    class={`palette-section palette-section-${section.kind}`}
                    role="group"
                    aria-label={section.ariaLabel}
                  >
                    <Show when={section.label}>
                      <div class="palette-section-label" aria-hidden="true">
                        {section.label}
                      </div>
                    </Show>
                    <For each={section.results}>
                      {(result) => {
                        const idx = createMemo(() => indexOf(result));
                        return (
                          <button
                            type="button"
                            tabindex="-1"
                            id={optionId(idx())}
                            role="option"
                            aria-selected={selected() === idx()}
                            class={`palette-item ${selected() === idx() ? "is-selected" : ""}`}
                            onClick={() => handleSelect(result)}
                            onMouseMove={() => setSelectedIndex(idx())}
                          >
                            <div class="palette-item-text">
                              <span class="palette-item-label">{result.label}</span>
                              <Show when={result.snippet}>
                                <span class="palette-item-snippet">
                                  <For each={result.snippet}>
                                    {(seg) => (
                                      <span classList={{ "is-match": seg.matched }}>{seg.text}</span>
                                    )}
                                  </For>
                                </span>
                              </Show>
                              <Show when={result.detail}>
                                <span class="palette-item-desc">{result.detail}</span>
                              </Show>
                            </div>
                            <Show when={result.line !== undefined}>
                              <span class="palette-item-line">L{result.line}</span>
                            </Show>
                            <Show when={section.showKbd}>
                              <Kbd binding={result.kbd} muted={!result.kbd} />
                            </Show>
                          </button>
                        );
                      }}
                    </For>
                    <Show when={section.hiddenCount > 0}>
                      <div class="palette-section-more">
                        Showing {section.results.length} of {section.total}
                      </div>
                    </Show>
                  </div>
                )}
              </For>
            </div>
          </Show>
          <div class="palette-live" role="status" aria-live="polite">
            {countLabel()}
          </div>
          <Show when={props.notice?.()}>
            {(notice) => <div class="palette-notice">{notice()}</div>}
          </Show>
        </div>
      </div>
    </Show>
  );
}
