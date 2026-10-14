import { For, Show, createEffect, createMemo, onCleanup } from "solid-js";
import { installFocusTrap } from "../../lib/focus-trap";
import { formatTokenCount } from "../../lib/token-estimate";
import { previewOf, PREVIEW_CHARS } from "../../lib/prompt-preview";
import { basename } from "../../lib/path";
import {
  contextPackStore,
  type PackKind,
  type PackSkipReason,
} from "../../stores/global/context-pack";
import { useWindow } from "../WindowProvider/WindowProvider";
import Button from "../Button/Button";
import "./ContextPackModal.css";

const REASONS: Record<PackSkipReason, string> = {
  too_large: "too large",
  not_text: "not text",
  not_downloaded: "not downloaded",
  unreadable: "could not be read",
};

function tokens(count: number): string {
  return `~${formatTokenCount(count)} tokens`;
}

/**
 * Copy files as prompt: the file in front with the files it links to,
 * its folder, or a tag, with the count and the text before anything is
 * copied.
 */
export default function ContextPackModal() {
  const win = useWindow();
  let dialogRef: HTMLDivElement | undefined;
  const store = contextPackStore;

  createEffect(() => {
    if (!store.isOpen() || !dialogRef) return;
    const teardown = installFocusTrap(dialogRef, {
      onEscape: () => store.close(),
      fallbackRestore: () => {
        win.editor.focusEditor();
        return null;
      },
    });
    onCleanup(teardown);
  });

  const preview = createMemo(() => {
    const pack = store.pack();
    return pack ? previewOf(pack.text) : null;
  });

  const folderName = () => {
    const path = store.folder();
    return path ? basename(path) : "";
  };

  function option(kind: PackKind, label: string) {
    return (
      <label class="context-pack-option">
        <input
          type="radio"
          name="context-pack-source"
          value={kind}
          checked={store.kind() === kind}
          onChange={() => store.choose(kind)}
        />
        {label}
      </label>
    );
  }

  return (
    <Show when={store.isOpen()}>
      <div class="context-pack-overlay" onClick={() => store.close()}>
        <div
          ref={dialogRef}
          class="context-pack-dialog"
          onClick={(e) => e.stopPropagation()}
          role="dialog"
          aria-modal="true"
          aria-labelledby="context-pack-title"
        >
          <div id="context-pack-title" class="context-pack-title">
            Copy files as prompt
          </div>
          <fieldset class="context-pack-sources" aria-label="Files">
            <Show when={store.file()}>
              {option("linked", "This file and the files it links to")}
            </Show>
            {option("folder", `Files in ${folderName()}`)}
            <div class="context-pack-tag-row">
              {option("tag", "Files tagged")}
              <select
                class="context-pack-tag"
                aria-label="Tag"
                disabled={store.tags().length === 0}
                value={store.tag() ?? ""}
                onChange={(e) => {
                  store.chooseTag(e.currentTarget.value);
                  store.choose("tag");
                }}
              >
                <Show
                  when={store.tags().length > 0}
                  fallback={<option value="">No tags in your folder</option>}
                >
                  <For each={store.tags()}>{(name) => <option value={name}>#{name}</option>}</For>
                </Show>
              </select>
            </div>
          </fieldset>

          <div class="context-pack-meta" aria-live="polite">
            <Show when={!store.busy()} fallback="Reading the files…">
              <Show when={store.pack()} fallback={store.failed() ? "Could not read the files." : ""}>
                {(pack) => (
                  <>
                    {pack().files.length} {pack().files.length === 1 ? "file" : "files"} ·{" "}
                    {tokens(pack().tokens)}
                  </>
                )}
              </Show>
            </Show>
          </div>

          <Show when={store.pack()}>
            {(pack) => (
              <div class="context-pack-body">
                <ul class="context-pack-files" aria-label="Files in the prompt">
                  <For each={pack().files}>
                    {(file) => (
                      <li class="context-pack-file">
                        <span class="context-pack-path">{file.path}</span>
                        <span class="context-pack-tokens">{tokens(file.tokens)}</span>
                      </li>
                    )}
                  </For>
                </ul>
                <Show when={pack().skipped.length > 0}>
                  <div class="context-pack-note">
                    Not included:{" "}
                    {pack()
                      .skipped.map((file) => `${file.path} (${REASONS[file.reason]})`)
                      .join(", ")}
                  </div>
                </Show>
                <Show when={pack().omitted > 0}>
                  <div class="context-pack-note">
                    {pack().omitted} more {pack().omitted === 1 ? "file was" : "files were"} left
                    out: a prompt holds up to 100 files and 4 MB.
                  </div>
                </Show>
                <Show when={preview()}>
                  {(shown) => (
                    <>
                      <pre class="context-pack-preview" aria-label="Preview" tabindex="0">
                        {shown().text}
                      </pre>
                      <Show when={shown().cut}>
                        <div class="context-pack-note">
                          Showing the first {PREVIEW_CHARS.toLocaleString()} characters. Copy takes
                          all of it.
                        </div>
                      </Show>
                    </>
                  )}
                </Show>
              </div>
            )}
          </Show>

          <div class="context-pack-actions">
            <Button class="context-pack-cancel" onClick={() => store.close()}>
              Cancel
            </Button>
            <Button
              variant="primary"
              class="context-pack-copy"
              disabled={store.busy() || (store.pack()?.files.length ?? 0) === 0}
              onClick={() => void store.copy()}
            >
              Copy
            </Button>
          </div>
        </div>
      </div>
    </Show>
  );
}
