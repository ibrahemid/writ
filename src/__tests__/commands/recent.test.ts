import { describe, it, expect, vi, beforeEach } from "vitest";

const h = vi.hoisted(() => ({
  showRecent: vi.fn<() => boolean>(),
  hasWindow: true,
}));

vi.mock("../../stores/global/window-registry", () => ({
  windowRegistry: {
    getActive: () => (h.hasWindow ? { sidebar: { showRecent: h.showRecent } } : null),
  },
}));

vi.mock("../../components/Notifications/Toast", () => ({
  showToast: vi.fn(),
}));

import { openRecent, NOTHING_CLOSED } from "../../commands/recent";
import { showToast } from "../../components/Notifications/Toast";

const mockedToast = vi.mocked(showToast);

beforeEach(() => {
  vi.clearAllMocks();
  h.hasWindow = true;
});

describe("Open Recent", () => {
  it("says nothing has been closed yet when Recent has no rows", () => {
    h.showRecent.mockReturnValue(false);

    openRecent();

    expect(h.showRecent).toHaveBeenCalledTimes(1);
    expect(mockedToast).toHaveBeenCalledWith(NOTHING_CLOSED, "info");
    expect(NOTHING_CLOSED).toBe("Nothing closed yet.");
  });

  it("opens Recent and says nothing more when a file has been closed", () => {
    h.showRecent.mockReturnValue(true);

    openRecent();

    expect(h.showRecent).toHaveBeenCalledTimes(1);
    expect(mockedToast).not.toHaveBeenCalled();
  });

  it("does nothing without a window", () => {
    h.hasWindow = false;

    openRecent();

    expect(h.showRecent).not.toHaveBeenCalled();
    expect(mockedToast).not.toHaveBeenCalled();
  });
});
