import { describe, it, expect, afterEach, beforeEach, vi } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import ToastContainer from "../../components/Notifications/Toast";
import {
  ACTION_TOAST_DURATION_MS,
  TOAST_DURATION_MS,
  dismissToast,
  showToast,
  toasts,
} from "../../stores/global/notifications";
import { installFocusTrap } from "../../lib/focus-trap";

// A toast can carry one action, an error stays until it is read and
// dismissed, a toast under the pointer or focus does not leave, and a toast
// raised from inside a modal is still reachable.

function clearAll() {
  for (const toast of toasts()) dismissToast(toast.id);
}

beforeEach(() => {
  vi.useFakeTimers();
  clearAll();
});

afterEach(() => {
  clearAll();
  cleanup();
  vi.useRealTimers();
  document.body.innerHTML = "";
});

describe("a toast with an action", () => {
  it("runs the action once, leaves, and reports that it ran", async () => {
    const run = vi.fn();
    const onClose = vi.fn();
    showToast({
      text: "Renamed to Plan. Updated links in 3 files.",
      type: "success",
      action: { label: "Undo", run },
      onClose,
    });
    const { getByRole, container } = render(() => <ToastContainer />);
    fireEvent.click(getByRole("button", { name: "Undo" }));
    await vi.runAllTimersAsync();
    expect(run).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledWith(true);
    expect(container.querySelector(".toast")).toBeNull();
  });

  it("reports a dismissal as the action not taken", () => {
    const onClose = vi.fn();
    const id = showToast({ text: "Cleared Recent", action: { label: "Undo", run: vi.fn() }, onClose });
    dismissToast(id);
    expect(onClose).toHaveBeenCalledWith(false);
  });

  it("stays longer than a plain toast, then leaves as not taken", () => {
    const onClose = vi.fn();
    showToast({ text: "Closed 4 tabs", action: { label: "Undo", run: vi.fn() }, onClose });
    vi.advanceTimersByTime(TOAST_DURATION_MS + 1);
    expect(toasts()).toHaveLength(1);
    vi.advanceTimersByTime(ACTION_TOAST_DURATION_MS);
    expect(toasts()).toHaveLength(0);
    expect(onClose).toHaveBeenCalledWith(false);
  });
});

describe("how long a toast stays", () => {
  it("keeps an error until it is dismissed, whatever duration the caller passed", () => {
    showToast("Could not move \"Draft\" to the Trash.", "error", 4000);
    vi.advanceTimersByTime(60_000);
    expect(toasts().map((t) => t.text)).toEqual(["Could not move \"Draft\" to the Trash."]);
  });

  it("lets an info line go after its time", () => {
    showToast("Saved", "info");
    vi.advanceTimersByTime(TOAST_DURATION_MS + 1);
    expect(toasts()).toHaveLength(0);
  });

  it("pauses under the pointer and resumes with the time that was left", () => {
    showToast("Saved", "info");
    const { container } = render(() => <ToastContainer />);
    const toast = container.querySelector(".toast")!;
    vi.advanceTimersByTime(TOAST_DURATION_MS - 500);
    fireEvent.pointerEnter(toast);
    vi.advanceTimersByTime(30_000);
    expect(toasts()).toHaveLength(1);
    fireEvent.pointerLeave(toast);
    vi.advanceTimersByTime(499);
    expect(toasts()).toHaveLength(1);
    vi.advanceTimersByTime(2);
    expect(toasts()).toHaveLength(0);
  });

  it("pauses while focus is inside it, across its own buttons", () => {
    showToast({ text: "Closed 2 tabs", action: { label: "Undo", run: vi.fn() } });
    const { container } = render(() => <ToastContainer />);
    const undo = container.querySelector<HTMLButtonElement>(".toast-action")!;
    const dismiss = container.querySelector<HTMLButtonElement>(".toast-dismiss")!;
    fireEvent.focusIn(undo);
    fireEvent.focusOut(undo, { relatedTarget: dismiss });
    fireEvent.focusIn(dismiss, { relatedTarget: undo });
    vi.advanceTimersByTime(60_000);
    expect(toasts()).toHaveLength(1);
    fireEvent.focusOut(dismiss, { relatedTarget: document.body });
    vi.advanceTimersByTime(ACTION_TOAST_DURATION_MS + 1);
    expect(toasts()).toHaveLength(0);
  });
});

describe("a toast raised inside a modal", () => {
  it("stays out of the modal's inert set", () => {
    showToast("Could not save the shortcuts", "error");
    const { container } = render(() => (
      <>
        <div class="modal-host">
          <div class="modal">
            <button type="button">Save</button>
          </div>
        </div>
        <ToastContainer />
      </>
    ));
    const modal = container.querySelector<HTMLElement>(".modal")!;
    const teardown = installFocusTrap(modal);
    const column = container.querySelector<HTMLElement>(".toast-container")!;
    expect(column.closest("[inert]")).toBeNull();
    teardown();
  });
});
