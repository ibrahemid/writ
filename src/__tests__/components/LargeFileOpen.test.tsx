import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup, fireEvent } from "@solidjs/testing-library";
import WindowProvider, { useWindow } from "../../components/WindowProvider/WindowProvider";
import LargeFileOpen from "../../components/LargeFileOpen/LargeFileOpen";
import { largeFileOpen } from "../../stores/global/large-file-open";
import { unreadTabs } from "../../stores/global/unread-tabs";

// The question states the size before anything reads the file; Open turns it
// into progress that lasts until the editor shows the text.

type Win = ReturnType<typeof useWindow>;

function mount(): { container: HTMLElement; win: () => Win } {
  let captured: Win | undefined;
  function Probe() {
    captured = useWindow();
    return null;
  }
  const result = render(() => (
    <WindowProvider windowId={9701}>
      <Probe />
      <LargeFileOpen />
    </WindowProvider>
  ));
  return { container: result.container, win: () => captured! };
}

function button(container: HTMLElement, label: string): HTMLButtonElement {
  const found = Array.from(container.querySelectorAll("button")).find(
    (b) => b.textContent?.trim() === label,
  );
  if (!found) throw new Error(`no ${label} button`);
  return found;
}

afterEach(() => {
  for (let i = 0; i < 20 && largeFileOpen.state() !== null; i++) {
    largeFileOpen.respond(false);
    largeFileOpen.finish();
  }
  unreadTabs.clear("big");
  cleanup();
});

describe("LargeFileOpen", () => {
  it("names the file and states its size before anything is read", () => {
    const { container } = mount();
    void largeFileOpen.ask("server.log", 60_000_000);

    expect(container.textContent).toContain("Open server.log?");
    expect(container.textContent).toContain("It is 60 MB.");
    expect(container.querySelector('[role="alertdialog"]')).not.toBeNull();
    expect(container.querySelector('[role="progressbar"]')).toBeNull();
  });

  it("answers Cancel as no, and the question is gone", async () => {
    const { container } = mount();
    const answer = largeFileOpen.ask("server.log", 60_000_000);

    fireEvent.click(button(container, "Cancel"));

    await expect(answer).resolves.toBe(false);
    expect(container.querySelector(".large-file-dialog")).toBeNull();
  });

  it("turns Open into progress in the same panel", async () => {
    const { container } = mount();
    const answer = largeFileOpen.ask("server.log", 60_000_000);

    fireEvent.click(button(container, "Open"));

    await expect(answer).resolves.toBe(true);
    expect(container.textContent).toContain("Opening server.log");
    expect(container.querySelector('[role="progressbar"]')).not.toBeNull();
    expect(container.querySelector('[aria-busy="true"]')).not.toBeNull();
    expect(container.querySelectorAll("button")).toHaveLength(0);
  });

  it("keeps the progress until the editor shows the file, then goes", async () => {
    const { container, win } = mount();
    const answer = largeFileOpen.ask("server.log", 60_000_000);
    fireEvent.click(button(container, "Open"));
    await answer;

    largeFileOpen.handOff("big");
    win().tabs.setActiveTabId("big");
    expect(container.querySelector('[role="progressbar"]')).not.toBeNull();

    win().editor.setCurrentBufferId("big");
    expect(container.querySelector(".large-file-dialog")).toBeNull();
    expect(largeFileOpen.state()).toBeNull();
  });

  it("goes when the editor could not read the file", async () => {
    const { container, win } = mount();
    const answer = largeFileOpen.ask("server.log", 60_000_000);
    fireEvent.click(button(container, "Open"));
    await answer;
    largeFileOpen.handOff("big");

    win().editor.failRead("big", "ERR_READ_PERMISSION_DENIED: denied");

    expect(container.querySelector(".large-file-dialog")).toBeNull();
  });

  // The file grew past the size Writ opens between Open and the editor's read,
  // which measured it and read nothing: the tab says why in the editor's place.
  it("goes when the editor's read found the file past the size Writ opens", async () => {
    const { container, win } = mount();
    const answer = largeFileOpen.ask("server.log", 60_000_000);
    fireEvent.click(button(container, "Open"));
    await answer;
    largeFileOpen.handOff("big");
    win().tabs.setActiveTabId("big");

    unreadTabs.takeRefused("big", "it is 600 MB, and Writ opens files up to 524 MB.");

    expect(container.querySelector(".large-file-dialog")).toBeNull();
    expect(largeFileOpen.state()).toBeNull();
  });

  it("goes when another tab takes the front before the editor showed it", async () => {
    const { container, win } = mount();
    const answer = largeFileOpen.ask("server.log", 60_000_000);
    fireEvent.click(button(container, "Open"));
    await answer;
    largeFileOpen.handOff("big");
    win().tabs.setActiveTabId("big");

    win().tabs.setActiveTabId("other");

    expect(container.querySelector(".large-file-dialog")).toBeNull();
  });

  it("keeps a second question until the first is answered, then asks it", async () => {
    const { container } = mount();
    const first = largeFileOpen.ask("a.log", 60_000_000);
    const second = largeFileOpen.ask("b.log", 70_000_000);

    expect(container.querySelector(".large-file-title")?.textContent).toBe("Open a.log?");
    const dialog = container.querySelector(".large-file-dialog");

    fireEvent.click(button(container, "Cancel"));
    await expect(first).resolves.toBe(false);
    expect(container.querySelector(".large-file-title")?.textContent).toBe("Open b.log?");
    // The dialog stays up between the two, so focus never drops to the editor.
    expect(container.querySelector(".large-file-dialog")).toBe(dialog);
    expect(container.textContent).toContain("It is 70 MB.");

    fireEvent.click(button(container, "Open"));
    await expect(second).resolves.toBe(true);
  });

  it("keeps a question asked during the progress until the file is shown", async () => {
    const { container, win } = mount();
    const first = largeFileOpen.ask("a.log", 60_000_000);
    fireEvent.click(button(container, "Open"));
    await first;
    largeFileOpen.handOff("big");

    const second = largeFileOpen.ask("b.log", 70_000_000);
    expect(container.querySelector(".large-file-title")?.textContent).toBe("Opening a.log");

    win().tabs.setActiveTabId("big");
    win().tabs.setActiveTabId("other");

    expect(container.querySelector(".large-file-title")?.textContent).toBe("Open b.log?");
    fireEvent.click(button(container, "Cancel"));
    await expect(second).resolves.toBe(false);
  });
});
