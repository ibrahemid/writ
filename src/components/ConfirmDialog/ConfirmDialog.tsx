import { For, Show, createEffect, onCleanup } from "solid-js";
import { installFocusTrap } from "../../lib/focus-trap";
import { hunkRows } from "../../lib/diff-rows";
import {
  initialFocus,
  pendingConfirm,
  settleConfirm,
} from "../../stores/global/dialogs";
import Button from "../Button/Button";
import { useWindow } from "../WindowProvider/WindowProvider";
import "./ConfirmDialog.css";

export default function ConfirmDialog() {
  const win = useWindow();
  let dialogRef: HTMLDivElement | undefined;
  let confirmRef: HTMLButtonElement | undefined;
  let cancelRef: HTMLButtonElement | undefined;

  createEffect(() => {
    const current = pendingConfirm();
    if (!current || !dialogRef) return;
    const teardown = installFocusTrap(dialogRef, {
      onEscape: () => settleConfirm("dismissed"),
      fallbackRestore: () => {
        win.editor.focusEditor();
        return null;
      },
    });
    const focusTarget = () => (initialFocus(current) === "cancel" ? cancelRef : confirmRef);
    requestAnimationFrame(() => focusTarget()?.focus());
    onCleanup(teardown);
  });

  return (
    <Show when={pendingConfirm()}>
      {(req) => (
        <div class="confirm-overlay" onClick={() => settleConfirm("dismissed")}>
          <div
            ref={dialogRef}
            class="confirm-dialog"
            classList={{ "has-diff": (req().hunks?.length ?? 0) > 0 }}
            onClick={(e) => e.stopPropagation()}
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="confirm-title"
            aria-describedby={req().message ? "confirm-message" : undefined}
          >
            <div id="confirm-title" class="confirm-title">
              {req().title}
            </div>
            <Show when={req().message}>
              <div id="confirm-message" class="confirm-message">
                {req().message}
              </div>
            </Show>
            <Show when={(req().hunks?.length ?? 0) > 0}>
              <div class="confirm-diff" tabindex="0" data-diff>
                <For each={(req().hunks ?? []).map(hunkRows)}>
                  {(hunk) => (
                    <div class="confirm-diff-hunk">
                      <For each={hunk}>
                        {(row) => (
                          <div class="confirm-diff-row" data-kind={row.kind}>
                            <span class="confirm-diff-mark" aria-hidden="true">
                              {row.kind === "added" ? "+" : row.kind === "removed" ? "-" : " "}
                            </span>
                            <span class="confirm-diff-text">{row.text}</span>
                          </div>
                        )}
                      </For>
                    </div>
                  )}
                </For>
              </div>
            </Show>
            <div class="confirm-actions">
              <Button
                ref={cancelRef}
                variant="secondary"
                class="confirm-cancel"
                onClick={() => settleConfirm("cancel")}
              >
                {req().cancelLabel ?? "Cancel"}
              </Button>
              <Button
                ref={confirmRef}
                variant="primary"
                danger={req().danger}
                class="confirm-accept"
                onClick={() => settleConfirm("confirm")}
              >
                {req().confirmLabel ?? "Confirm"}
              </Button>
            </div>
          </div>
        </div>
      )}
    </Show>
  );
}
