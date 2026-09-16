import { For, Show, createEffect, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import Icon from "../Icon/Icon";
import { configStore } from "../../stores/global/config";
import { aiConnectionStore, modelListDisplay } from "../../stores/global/ai-connection";
import { aiProvidersStore } from "../../stores/global/ai-providers";
import { aiRewriteStore } from "../../stores/global/ai-rewrite";
import { modelOptions } from "../../stores/global/ai-models";
import { openSettings } from "../SettingsModal/SettingsModal";

// Singleton state — Writ is single-window. A failed reply offers to change the
// model, and the control it opens is the one in the composer below it.
const [openRequests, setOpenRequests] = createSignal(0);

/** Opens the connection control, for a failure that names the model. */
export function openConnectionControl(): void {
  setOpenRequests((count) => count + 1);
}

/**
 * Which connection answers, and the model it answers with.
 *
 * Both choices go through `aiConnectionStore`, so this control, the settings
 * panel and the send preflight read one catalog and one config. A provider
 * change drops a model that belonged to the provider before it, because Rust
 * does that in the one place it happens (ADR-040 section 1).
 */
export default function ChatConnectionControl() {
  const [open, setOpen] = createSignal(false);
  const [probe, setProbe] = createSignal<{ ollama: boolean; lmstudio: boolean } | null>(null);
  const [keys, setKeys] = createSignal<Record<string, boolean>>({});
  let button: HTMLButtonElement | undefined;
  let menu: HTMLDivElement | undefined;
  let root: HTMLDivElement | undefined;

  const ai = () => configStore.config().ai;
  const provider = () => aiProvidersStore.byId(ai().provider);
  const providerLabel = () => provider()?.label ?? ai().provider;

  /** The model this chat sends: its own choice while that choice belongs to
   * the connection's provider, and the connection's model otherwise. */
  const model = () => {
    const chat = ai().chat;
    const override = chat.model.trim();
    return override && chat.model_provider === ai().provider ? override : ai().model;
  };

  const catalog = () => aiConnectionStore.catalog();
  const live = () => (catalog()?.source === "live" ? (catalog()?.models ?? []) : []);
  const suggested = () => live().length === 0;

  /** The ids on offer, plus the saved one when the connection's own list does
   * not hold it: a model nobody can reach is named rather than hidden. */
  const models = createMemo(() => {
    const ids = modelOptions(ai().provider, live());
    const saved = model().trim();
    if (saved && !ids.includes(saved)) return [...ids, saved];
    return ids;
  });

  const unavailable = (id: string) => live().length > 0 && !live().includes(id);

  /** What the model list says when it holds nothing to pick from. */
  const listNote = (): string => {
    if (aiConnectionStore.listing()) return "Reading the model list.";
    const held = catalog();
    if (held?.error) return modelListDisplay(held.error, providerLabel()).text;
    if (held?.source === "live" && held.models.length === 0) {
      return `${providerLabel()} listed no models.`;
    }
    return models().length === 0 ? `${providerLabel()} listed no models.` : "";
  };

  /** What a provider row still needs, in its own words. */
  function providerNote(row: { id: string; group: string; needs_key: boolean }): string {
    if (row.group === "local") {
      const answered = probe();
      if (!answered) return "";
      const running = row.id === "lmstudio" ? answered.lmstudio : answered.ollama;
      return running ? "Running" : "Not running";
    }
    if (!row.needs_key) return "";
    const state = keys()[row.id];
    if (state === undefined) return "";
    return state ? "Key set" : "No key";
  }

  function close(returnFocus: boolean) {
    setOpen(false);
    if (returnFocus) button?.focus();
  }

  // Opened from elsewhere: a failure that names the model offers to change it.
  let seen = openRequests();
  createEffect(() => {
    const asked = openRequests();
    if (asked === seen) return;
    seen = asked;
    setOpen(true);
  });

  // What each row still needs is read when the list is opened, not held while
  // the pane sits idle: a key can be added in another window.
  createEffect(() => {
    if (!open()) return;
    void aiProvidersStore.load();
    void aiConnectionStore
      .probeLocal()
      .then(setProbe)
      .catch(() => setProbe(null));
    for (const row of aiProvidersStore.rows()) {
      if (!row.needs_key) continue;
      void aiRewriteStore
        .hasApiKey(row.id)
        .then((state) => setKeys((held) => ({ ...held, [row.id]: state.is_set })))
        .catch(() => undefined);
    }
  });

  // The rows take focus as they are walked, so the list is left through the
  // same key that opened it rather than by tabbing out of the column.
  createEffect(() => {
    if (!open()) return;
    queueMicrotask(() => rows()[0]?.focus());
  });

  function rows(): HTMLButtonElement[] {
    if (!menu) return [];
    return Array.from(menu.querySelectorAll<HTMLButtonElement>(".chat-menu-row:not([disabled])"));
  }

  function step(by: number) {
    const all = rows();
    if (all.length === 0) return;
    const at = all.indexOf(document.activeElement as HTMLButtonElement);
    const next = at < 0 ? 0 : (at + by + all.length) % all.length;
    all[next].focus();
  }

  function onMenuKeyDown(event: KeyboardEvent) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      step(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      step(-1);
    } else if (event.key === "Escape") {
      event.preventDefault();
      close(true);
    }
  }

  function onPointerDown(event: PointerEvent) {
    if (!open()) return;
    const target = event.target;
    if (target instanceof Node && root?.contains(target)) return;
    close(false);
  }

  onMount(() => {
    document.addEventListener("pointerdown", onPointerDown);
  });

  onCleanup(() => {
    document.removeEventListener("pointerdown", onPointerDown);
  });

  async function chooseProvider(id: string) {
    close(true);
    if (id === ai().provider) return;
    await aiConnectionStore.selectProvider(id);
  }

  async function chooseModel(id: string) {
    close(true);
    // The connection's own model is not an override; picking it puts the chat
    // back on whatever the connection is set to.
    await aiConnectionStore.selectChatModel(id === ai().model ? null : id);
  }

  return (
    <div class="chat-connection-wrap" ref={root}>
      <button
        type="button"
        class="chat-connection"
        ref={button}
        aria-haspopup="true"
        aria-expanded={open()}
        onClick={() => (open() ? close(true) : setOpen(true))}
      >
        <span class="chat-connection-label">
          {providerLabel()} · {model() || "No model"}
        </span>
        <Icon name="caret-down" size={10} />
      </button>

      <Show when={open()}>
        <div
          class="chat-connection-menu"
          role="menu"
          aria-label="Model connection"
          ref={menu}
          onKeyDown={onMenuKeyDown}
        >
          <div class="chat-menu-group" role="group" aria-label="Provider">
            <p class="chat-menu-label" aria-hidden="true">
              Provider
            </p>
            <For each={aiProvidersStore.rows()}>
              {(row) => (
                <button
                  type="button"
                  role="menuitem"
                  class="chat-menu-row"
                  classList={{ "is-current": row.id === ai().provider }}
                  aria-current={row.id === ai().provider ? "true" : undefined}
                  onClick={() => void chooseProvider(row.id)}
                >
                  <span class="chat-menu-mark" aria-hidden="true">
                    <Show when={row.id === ai().provider}>
                      <Icon name="check" size={11} />
                    </Show>
                  </span>
                  <span class="chat-menu-name">{row.label}</span>
                  <span class="chat-menu-note">{providerNote(row)}</span>
                </button>
              )}
            </For>
          </div>

          <div class="chat-menu-group" role="group" aria-label="Model">
            <p class="chat-menu-label" aria-hidden="true">
              Model
            </p>
            <For each={models()}>
              {(id) => (
                <button
                  type="button"
                  role="menuitem"
                  class="chat-menu-row"
                  classList={{ "is-current": id === model(), "is-off": unavailable(id) }}
                  disabled={unavailable(id)}
                  aria-current={id === model() ? "true" : undefined}
                  onClick={() => void chooseModel(id)}
                >
                  <span class="chat-menu-mark" aria-hidden="true">
                    <Show when={id === model()}>
                      <Icon name="check" size={11} />
                    </Show>
                  </span>
                  <span class="chat-menu-name">{id}</span>
                  <span class="chat-menu-note">
                    {unavailable(id)
                      ? `not available on ${providerLabel()}`
                      : suggested()
                        ? "suggested"
                        : ""}
                  </span>
                </button>
              )}
            </For>
            <Show when={listNote().length > 0}>
              <p class="chat-menu-empty">{listNote()}</p>
            </Show>
          </div>

          <button
            type="button"
            role="menuitem"
            class="chat-menu-row chat-menu-settings"
            onClick={() => {
              close(false);
              openSettings("ai", "ai.provider");
            }}
          >
            <span class="chat-menu-mark" aria-hidden="true" />
            <span class="chat-menu-name">AI settings</span>
          </button>
        </div>
      </Show>
    </div>
  );
}
