import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installGuestScrollIntoView } from "../guest-scroll";

interface Box {
  top: number;
  left: number;
  width: number;
  height: number;
}

interface FakeScroller {
  element: HTMLElement;
  scrollTo: ReturnType<typeof vi.fn>;
}

const PORT: Box = { top: 100, left: 50, width: 400, height: 200 };

function placeAt(element: Element, box: Box): void {
  element.getBoundingClientRect = () =>
    ({ ...box, x: box.left, y: box.top, right: box.left + box.width, bottom: box.top + box.height, toJSON: () => box }) as DOMRect;
}

function makeScroller(element: HTMLElement, options: { style: string; box: Box; scrollWidth: number; scrollHeight: number }): FakeScroller {
  element.setAttribute("style", options.style);
  placeAt(element, options.box);
  Object.defineProperties(element, {
    clientWidth: { value: options.box.width, configurable: true },
    clientHeight: { value: options.box.height, configurable: true },
    scrollWidth: { value: options.scrollWidth, configurable: true },
    scrollHeight: { value: options.scrollHeight, configurable: true },
    scrollTop: { value: 0, writable: true, configurable: true },
    scrollLeft: { value: 0, writable: true, configurable: true },
  });
  const scrollTo = vi.fn((position: ScrollToOptions) => {
    element.scrollTop = position.top ?? element.scrollTop;
    element.scrollLeft = position.left ?? element.scrollLeft;
  });
  element.scrollTo = scrollTo as unknown as HTMLElement["scrollTo"];
  return { element, scrollTo };
}

describe("the guest scrollIntoView", () => {
  const priorScrollIntoView = Object.getOwnPropertyDescriptor(Element.prototype, "scrollIntoView");
  const browserScrollIntoView = vi.fn();
  let isGuest = true;
  let uninstall: () => void;
  let windowScrollTo: ReturnType<typeof vi.spyOn>;
  let windowScrollBy: ReturnType<typeof vi.spyOn>;
  let pageScrollTo: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    Element.prototype.scrollIntoView = browserScrollIntoView;
    isGuest = true;
    uninstall = installGuestScrollIntoView(() => isGuest);
    windowScrollTo = vi.spyOn(window, "scrollTo").mockImplementation(() => {});
    windowScrollBy = vi.spyOn(window, "scrollBy").mockImplementation(() => {});
    pageScrollTo = vi.fn();
    document.documentElement.scrollTo = pageScrollTo as unknown as HTMLElement["scrollTo"];
    document.body.scrollTo = pageScrollTo as unknown as HTMLElement["scrollTo"];
    document.body.setAttribute("style", "overflow-x: auto; overflow-y: auto");
  });

  afterEach(() => {
    uninstall();
    if (priorScrollIntoView) Object.defineProperty(Element.prototype, "scrollIntoView", priorScrollIntoView);
    else delete (Element.prototype as { scrollIntoView?: unknown }).scrollIntoView;
    browserScrollIntoView.mockReset();
    vi.restoreAllMocks();
    document.body.removeAttribute("style");
    document.body.innerHTML = "";
    delete (document.documentElement as { scrollTo?: unknown }).scrollTo;
    delete (document.body as { scrollTo?: unknown }).scrollTo;
  });

  function mountTab(target: Box, style = "overflow-x: auto; overflow-y: auto") {
    document.body.innerHTML = `<div class="strip"><button class="tab">Garden committee</button></div>`;
    const strip = makeScroller(document.querySelector<HTMLElement>(".strip")!, { style, box: PORT, scrollWidth: 2000, scrollHeight: 1000 });
    const tab = document.querySelector<HTMLElement>(".tab")!;
    placeAt(tab, target);
    return { strip, tab };
  }

  function expectPageUntouched(): void {
    expect(windowScrollTo).not.toHaveBeenCalled();
    expect(windowScrollBy).not.toHaveBeenCalled();
    expect(pageScrollTo).not.toHaveBeenCalled();
    expect(browserScrollIntoView).not.toHaveBeenCalled();
  }

  it("scrolls the nearest way on both axes inside the frame only", () => {
    const { strip, tab } = mountTab({ top: 350, left: 500, width: 60, height: 30 });
    tab.scrollIntoView({ behavior: "smooth", block: "nearest", inline: "nearest" });
    expect(strip.scrollTo).toHaveBeenCalledWith({ top: 80, left: 110, behavior: "smooth" });
    expect(strip.element.scrollTop).toBe(80);
    expect(strip.element.scrollLeft).toBe(110);
    expectPageUntouched();
  });

  it("leaves a scroller alone when the element already shows the nearest way", () => {
    const { strip, tab } = mountTab({ top: 150, left: 100, width: 60, height: 30 });
    tab.scrollIntoView({ block: "nearest", inline: "nearest" });
    expect(strip.scrollTo).not.toHaveBeenCalled();
    expectPageUntouched();
  });

  it("aligns to the start on both axes", () => {
    const { strip, tab } = mountTab({ top: 350, left: 500, width: 60, height: 30 });
    tab.scrollIntoView({ block: "start", inline: "start" });
    expect(strip.scrollTo).toHaveBeenCalledWith({ top: 250, left: 450, behavior: "auto" });
    expectPageUntouched();
  });

  it("centres, as the settings dialog asks for its section", () => {
    const { strip, tab } = mountTab({ top: 350, left: 500, width: 60, height: 30 });
    tab.scrollIntoView({ block: "center", inline: "center" });
    expect(strip.scrollTo).toHaveBeenCalledWith({ top: 165, left: 280, behavior: "auto" });
    expectPageUntouched();
  });

  it("reads no argument as start and false as end", () => {
    const { strip, tab } = mountTab({ top: 350, left: 500, width: 60, height: 30 });
    tab.scrollIntoView();
    expect(strip.scrollTo).toHaveBeenLastCalledWith({ top: 250, left: 110, behavior: "auto" });
    strip.element.scrollTop = 0;
    strip.element.scrollLeft = 0;
    tab.scrollIntoView(false);
    expect(strip.scrollTo).toHaveBeenLastCalledWith({ top: 80, left: 110, behavior: "auto" });
    expectPageUntouched();
  });

  it("keeps each scroll inside the scroller's range and moves only the axes it scrolls", () => {
    const { strip, tab } = mountTab({ top: 5000, left: 5000, width: 60, height: 30 }, "overflow-x: hidden; overflow-y: visible");
    tab.scrollIntoView({ block: "start", inline: "start" });
    expect(strip.scrollTo).toHaveBeenCalledWith({ top: 0, left: 1600, behavior: "auto" });
    expectPageUntouched();
  });

  it("reads start and end from the right in a right-to-left scroller", () => {
    const { strip, tab } = mountTab({ top: 150, left: 100, width: 60, height: 30 }, "overflow-x: auto; overflow-y: hidden; direction: rtl");
    tab.scrollIntoView({ block: "nearest", inline: "start" });
    expect(strip.scrollTo).toHaveBeenCalledWith({ top: 0, left: -290, behavior: "auto" });
    expectPageUntouched();
  });

  it("scrolls an outer scroller by where the element lands after the inner one moved", () => {
    document.body.innerHTML = `<div class="pane"><div class="strip"><button class="tab">To do</button></div></div>`;
    const pane = makeScroller(document.querySelector<HTMLElement>(".pane")!, {
      style: "overflow-y: auto; overflow-x: visible",
      box: { top: 0, left: 0, width: 800, height: 300 },
      scrollWidth: 800,
      scrollHeight: 3000,
    });
    const strip = makeScroller(document.querySelector<HTMLElement>(".strip")!, {
      style: "overflow-y: auto; overflow-x: visible",
      box: { top: 400, left: 0, width: 800, height: 100 },
      scrollWidth: 800,
      scrollHeight: 600,
    });
    const tab = document.querySelector<HTMLElement>(".tab")!;
    placeAt(tab, { top: 650, left: 0, width: 60, height: 30 });
    tab.scrollIntoView({ block: "nearest" });
    expect(strip.element.scrollTop).toBe(180);
    expect(pane.element.scrollTop).toBe(200);
    expectPageUntouched();
  });

  it("hands the call to the browser once the visitor engages", () => {
    const { strip, tab } = mountTab({ top: 350, left: 500, width: 60, height: 30 });
    isGuest = false;
    const options: ScrollIntoViewOptions = { block: "nearest", inline: "nearest" };
    tab.scrollIntoView(options);
    expect(browserScrollIntoView).toHaveBeenCalledTimes(1);
    expect(browserScrollIntoView.mock.contexts[0]).toBe(tab);
    expect(browserScrollIntoView).toHaveBeenCalledWith(options);
    expect(strip.scrollTo).not.toHaveBeenCalled();
  });

  it("puts the browser's method back on uninstall", () => {
    uninstall();
    expect(Element.prototype.scrollIntoView).toBe(browserScrollIntoView);
    uninstall = installGuestScrollIntoView(() => isGuest);
  });
});
