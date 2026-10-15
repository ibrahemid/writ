import { describe, it, expect, afterEach, vi } from "vitest";
import { BlockRenderError, createBlockRenderer } from "../../editor/block-renderer";
import { sanitizedFragment } from "../../editor/sanitize-markup";

function everyElement(root: DocumentFragment): Element[] {
  const found: Element[] = [];
  const walk = (parent: Element | DocumentFragment) => {
    for (const child of Array.from(parent.children)) {
      found.push(child);
      walk(child);
    }
  };
  walk(root);
  return found;
}

const CSS_LOAD = /url\s*\((?!\s*["']?#)|image-set\s*\(|@import/i;

/** True when nothing in the fragment could run, load or navigate. */
function inert(fragment: DocumentFragment): boolean {
  return everyElement(fragment).every((element) => {
    const name = element.localName.toLowerCase();
    if (["script", "iframe", "object", "embed", "animate", "set", "link"].includes(name)) return false;
    if (name === "style" && CSS_LOAD.test(element.textContent ?? "")) return false;
    return Array.from(element.attributes).every((attribute) => {
      const attr = attribute.name.toLowerCase();
      if (attr.startsWith("on")) return false;
      if (CSS_LOAD.test(attribute.value)) return false;
      if (attr === "srcset") return false;
      if (["href", "xlink:href", "src", "action", "formaction"].includes(attr)) {
        return attribute.value.trim().startsWith("#");
      }
      return true;
    });
  });
}

const HOSTILE_SVG = [
  '<svg viewBox="0 0 10 10" onload="alert(1)">',
  "<style>.node { fill: red; }</style>",
  "<style>@import url(https://evil.example/x.css); .edge { fill: blue; }</style>",
  "<script>alert(1)</script>",
  '<a href="javascript:alert(1)"><text onclick="alert(2)">t</text></a>',
  '<a xlink:href="javascript:alert(3)"><animate attributeName="href" values="javascript:alert(4)"></animate></a>',
  '<use href="https://evil.example/sprite.svg#x"></use>',
  '<path marker-end="url(#arrow)" d="M0 0L1 1"></path>',
  '<foreignObject><div><img src="x" onerror="alert(5)">label</div></foreignObject>',
  "</svg>",
].join("");

describe("sanitizing what a renderer drew", () => {
  it("leaves nothing that runs, loads or navigates", () => {
    const clean = sanitizedFragment(HOSTILE_SVG);
    expect(inert(clean)).toBe(true);
    const text = new XMLSerializer().serializeToString(clean);
    expect(text).not.toContain("javascript:");
    expect(text).not.toContain("@import");
    expect(text).not.toContain("evil.example");
  });

  it("keeps the drawing itself", () => {
    const root = sanitizedFragment(HOSTILE_SVG);
    expect(root.querySelector("svg")?.getAttribute("viewBox")).toBe("0 0 10 10");
    expect(Array.from(root.querySelectorAll("style"), (style) => style.textContent)).toEqual([".node { fill: red; }"]);
    expect(root.querySelector("path")?.getAttribute("marker-end")).toBe("url(#arrow)");
    expect(root.querySelector("foreignObject div")?.textContent).toBe("label");
  });

  it.each([
    ["a url() in a style element", "<svg><style>rect{fill:url(https://example.invalid/p.png)}</style></svg>"],
    ["a url() in a fill", '<svg><rect fill="url(https://example.invalid/q#g)"></rect></svg>'],
    ["an import put back together by stripping it", "<svg><style>@im@import a;port url(https://example.invalid/i.css);</style></svg>"],
    ["an import of a bare string", '<svg><style>@import "https://example.invalid/s.css";</style></svg>'],
    ["an escaped url() in a style element", "<svg><style>rect{fill:\\75 rl(https://example.invalid/e.png)}</style></svg>"],
    ["an escaped import", '<svg><style>@\\69mport "https://example.invalid/e.css";</style></svg>'],
    ["an escaped url() in a fill", '<svg><rect fill="u\\72l(https://example.invalid/f.png)"></rect></svg>'],
    ["an image-set() in a style attribute", `<div style="background-image:image-set('https://example.invalid/a.png' 1x)"></div>`],
    ["an image-set() in a style element", '<svg><style>g{background:-webkit-image-set("https://example.invalid/b.png" 1x)}</style></svg>'],
    [
      "url() in filter, clip-path, mask and marker-start",
      '<svg><g filter="url(https://example.invalid/a#f)" clip-path="url(\'https://example.invalid/b#c\')" ' +
        'mask="url(&quot;https://example.invalid/c#m&quot;)" marker-start="url(https://example.invalid/d#s)"></g></svg>',
    ],
    ["a url() split by an element inside a style", "<svg><style>rect{fill:ur<g>x</g>l(https://example.invalid/k.png)}</style></svg>"],
    ["a hex escape followed by CR LF in a style element", "<svg><style>rect{fill:\\75\r\nrl(https://example.invalid/n.png)}</style></svg>"],
    ["a hex escape followed by CR LF in a fill", '<svg><rect fill="\\75&#13;&#10;rl(https://example.invalid/o.png)"></rect></svg>'],
    ["a hex escape followed by FF in a style attribute", '<svg><rect style="fill:\\75\frl(https://example.invalid/p.png)"></rect></svg>'],
    ["a srcset whose second candidate leaves", "<svg><foreignObject><img srcset=\"#a 1x, https://example.invalid/m.png 2x\"></foreignObject></svg>"],
  ])("lets no URL out through %s", (_case, markup) => {
    const clean = sanitizedFragment(markup);
    expect(inert(clean)).toBe(true);
    expect(new XMLSerializer().serializeToString(clean)).not.toContain("example.invalid");
  });

  it("takes no url() written with escapes at its word", () => {
    const root = sanitizedFragment("<svg><style>rect{fill:u\\72l(\\22 #g\\22 )}</style></svg>");
    expect(root.querySelector("style")).toBeNull();
    const odd = sanitizedFragment('<svg><rect fill="\\ffffff \\0 \\d800 red"></rect></svg>');
    expect(odd.querySelector("rect")?.getAttribute("fill")).toBe("\\ffffff \\0 \\d800 red");
  });

  it("drops a whole style element that loads, and keeps one that only points inside the drawing", () => {
    const root = sanitizedFragment(
      "<svg><style>.a{fill:red}@import url(https://example.invalid/x.css);.b{fill:blue}</style>" +
        "<style>.node rect{fill:url(#grad)} .edge{marker-end:url( '#arrow' )}</style></svg>",
    );
    expect(Array.from(root.querySelectorAll("style"), (style) => style.textContent)).toEqual([
      ".node rect{fill:url(#grad)} .edge{marker-end:url( '#arrow' )}",
    ]);
  });

  it("keeps references to the drawing's own filters, clips, masks and markers", () => {
    const root = sanitizedFragment(
      '<svg><g filter="url(#f)" clip-path="url(\'#c\')" mask="url(&quot;#m&quot;)" marker-start="url( #s )"></g></svg>',
    );
    const group = root.querySelector("g");
    expect(group?.getAttribute("filter")).toBe("url(#f)");
    expect(group?.getAttribute("clip-path")).toBe("url('#c')");
    expect(group?.getAttribute("mask")).toBe('url("#m")');
    expect(group?.getAttribute("marker-start")).toBe("url( #s )");
  });
});

describe("math", () => {
  it("typesets with KaTeX and refuses commands that reach outside the expression", async () => {
    const renderer = createBlockRenderer();
    const markup = await renderer.renderMath("x^2", false);
    expect(markup).toContain("katex");
    for (const tex of [
      "\\href{javascript:alert(1)}{x}",
      "\\url{javascript:alert(1)}",
      "\\htmlClass{x}{y} \\htmlData{onclick=alert(1)}{z}",
      "\\text{<script>alert(1)</script>}",
      "\\includegraphics{https://evil.example/a.png}",
    ]) {
      const hostile = sanitizedFragment(await renderer.renderMath(tex, true));
      expect(inert(hostile), tex).toBe(true);
      const text = new XMLSerializer().serializeToString(hostile);
      expect(text).not.toContain("<script");
      expect(text).not.toContain('href="javascript');
      expect(text).not.toContain('src="https://evil');
    }
  });

  it("draws one expression once however often it is asked for", () => {
    const renderer = createBlockRenderer();
    expect(renderer.renderMath("a+b", true)).toBe(renderer.renderMath("a+b", true));
    expect(renderer.renderMath("a+b", true)).not.toBe(renderer.renderMath("a+b", false));
  });
});

interface FakeMermaid {
  initialize: ReturnType<typeof vi.fn>;
  parse: ReturnType<typeof vi.fn>;
  render: ReturnType<typeof vi.fn>;
}

function installMermaid(fake: FakeMermaid): void {
  (window as unknown as { mermaid?: FakeMermaid }).mermaid = fake;
}

afterEach(() => {
  delete (window as unknown as { mermaid?: unknown }).mermaid;
});

describe("diagrams", () => {
  it("draws one at a time, strict, in the theme asked for", async () => {
    let drawing = 0;
    let overlapped = false;
    const fake: FakeMermaid = {
      initialize: vi.fn(),
      parse: vi.fn(async () => true),
      render: vi.fn(async (id: string) => {
        drawing++;
        if (drawing > 1) overlapped = true;
        await new Promise((resolve) => setTimeout(resolve, 5));
        drawing--;
        return { svg: `<svg id="${id}" onclick="alert(1)"><g></g></svg>` };
      }),
    };
    installMermaid(fake);
    const renderer = createBlockRenderer();
    const [light, dark] = await Promise.all([
      renderer.renderDiagram("graph TD\nA-->B", "light"),
      renderer.renderDiagram("graph TD\nA-->C", "dark"),
    ]);
    expect(overlapped).toBe(false);
    expect(fake.initialize).toHaveBeenNthCalledWith(1, expect.objectContaining({ securityLevel: "strict", theme: "default" }));
    expect(fake.initialize).toHaveBeenNthCalledWith(2, expect.objectContaining({ securityLevel: "strict", theme: "dark" }));
    expect(inert(sanitizedFragment(light))).toBe(true);
    expect(dark).toContain("<svg");
  });

  it("names the line a diagram's error is on", async () => {
    installMermaid({
      initialize: vi.fn(),
      parse: vi.fn(async () => {
        throw new Error("Parse error on line 3:\n...A-->\n---^");
      }),
      render: vi.fn(),
    });
    const renderer = createBlockRenderer();
    const failure = await renderer.renderDiagram("graph TD\nA-->B\nA-->", "light").catch((e: unknown) => e);
    expect(failure).toBeInstanceOf(BlockRenderError);
    expect((failure as BlockRenderError).line).toBe(3);
  });
});
