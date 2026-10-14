import { describe, it, expect, vi } from "vitest";
import { render } from "@solidjs/testing-library";
import WindowProvider, { useWindow } from "../../components/WindowProvider/WindowProvider";
import type { WindowState } from "../../stores/window/createWindowState";

// A window drawn again after a crash builds a new editor store, so the one
// being torn down stops answering saves (F16).

describe("a window that goes away", () => {
  it("stops its editor store's save listener", () => {
    let win: WindowState | undefined;
    function Capture() {
      win = useWindow();
      return null;
    }
    const screen = render(() => (
      <WindowProvider windowId={9911}>
        <Capture />
      </WindowProvider>
    ));
    const stop = vi.fn();
    win!.editor.stopSaveListener = stop;

    screen.unmount();

    expect(stop).toHaveBeenCalledOnce();
  });
});
