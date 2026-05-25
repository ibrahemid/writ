import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import ToastContainer, {
  showToast,
  dismissToast,
} from "../../components/Notifications/Toast";

describe("Toast dismiss button (#48)", () => {
  afterEach(() => {
    document.querySelectorAll(".toast").forEach((el) => {
      const button = el.querySelector<HTMLButtonElement>(".toast-dismiss");
      if (button) button.click();
    });
    cleanup();
  });

  it("exposes accessible name 'Dismiss notification'", () => {
    showToast("hello", "info", 0);
    const { container } = render(() => <ToastContainer />);
    const dismiss = container.querySelector<HTMLButtonElement>(".toast-dismiss");
    expect(dismiss).not.toBeNull();
    expect(dismiss!.tagName).toBe("BUTTON");
    expect(dismiss!.getAttribute("type")).toBe("button");
    expect(dismiss!.getAttribute("aria-label")).toBe("Dismiss notification");
  });

  it("clicking dismiss removes the toast", () => {
    const id = showToast("removable", "warning", 0);
    const { container } = render(() => <ToastContainer />);
    expect(container.querySelector(".toast")).not.toBeNull();
    const dismiss = container.querySelector<HTMLButtonElement>(".toast-dismiss")!;
    fireEvent.click(dismiss);
    expect(container.querySelector(".toast")).toBeNull();
    dismissToast(id);
  });
});
