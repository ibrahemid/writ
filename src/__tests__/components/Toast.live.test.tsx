import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import ToastContainer, { showToast, dismissToast } from "../../components/Notifications/Toast";

describe("Toast live region (#49)", () => {
  const created: number[] = [];

  afterEach(() => {
    while (created.length) dismissToast(created.pop()!);
    cleanup();
  });

  it("container is a polite live region at mount", () => {
    const { container } = render(() => <ToastContainer />);
    const wrapper = container.querySelector<HTMLElement>(".toast-container");
    expect(wrapper).not.toBeNull();
    expect(wrapper!.getAttribute("aria-live")).toBe("polite");
  });

  it("error toast carries role=alert", () => {
    const { container } = render(() => <ToastContainer />);
    created.push(showToast("autosave failed", "error", 0));
    const errorToast = container.querySelector<HTMLElement>(".toast-error");
    expect(errorToast).not.toBeNull();
    expect(errorToast!.getAttribute("role")).toBe("alert");
  });

  it("non-error toasts carry role=status (announced via the polite container)", () => {
    const { container } = render(() => <ToastContainer />);
    created.push(showToast("saved", "success", 0));
    created.push(showToast("note", "info", 0));
    created.push(showToast("be careful", "warning", 0));

    const success = container.querySelector<HTMLElement>(".toast-success")!;
    const info = container.querySelector<HTMLElement>(".toast-info")!;
    const warning = container.querySelector<HTMLElement>(".toast-warning")!;

    expect(success.getAttribute("role")).toBe("status");
    expect(info.getAttribute("role")).toBe("status");
    expect(warning.getAttribute("role")).toBe("status");
  });
});
