// Elements that run, load or submit something, and the SVG animation
// elements, which can rewrite an attribute such as `href` after this pass.
const BLOCKED_ELEMENTS = new Set([
  "script",
  "iframe",
  "frame",
  "frameset",
  "object",
  "embed",
  "applet",
  "link",
  "meta",
  "base",
  "form",
  "input",
  "button",
  "textarea",
  "select",
  "audio",
  "video",
  "source",
  "track",
  "portal",
  "animate",
  "animatemotion",
  "animatetransform",
  "set",
  "discard",
]);

const URL_ATTRIBUTES = new Set([
  "href",
  "xlink:href",
  "src",
  "action",
  "formaction",
  "background",
  "poster",
  "data",
  "ping",
]);

// A fragment link inside the drawing, or an inline raster.
const SAFE_URL = /^(#|data:image\/(png|gif|jpe?g|webp);)/i;

// CSS that fetches: a url() other than a fragment of the drawing, the
// functions that take a bare string as an image, an import.
const CSS_FETCH = /url\s*\((?!\s*["']?#)|image-set\s*\(|image\s*\(|src\s*\(|@import|expression\s*\(/;
const CSS_URL = /url\s*\(/;
const CSS_ESCAPE = /\\(?:([0-9a-f]{1,6})[ \t\n\r\f]?|([\s\S]))/gi;

function decodeCssEscapes(text: string): string {
  // CSS turns CR LF, CR and FF into one LF before it reads escapes, so a hex
  // escape followed by CR LF swallows the pair as its one whitespace.
  const normalized = text.replace(/\r\n|\r|\f/g, "\n");
  return normalized.replace(CSS_ESCAPE, (_escape, hex: string | undefined, char: string | undefined) => {
    if (hex === undefined) return char ?? "";
    const code = Number.parseInt(hex, 16);
    const invalid = code === 0 || code > 0x10ffff || (code >= 0xd800 && code <= 0xdfff);
    return invalid ? "\uFFFD" : String.fromCodePoint(code);
  });
}

/**
 * Whether CSS text could load anything, judged on the text as a CSS parser
 * reads it, escapes decoded. Text that fails is dropped whole, never
 * rewritten. A decoded escape can pass for a quote, so a url() in text with
 * escapes is refused even when it seems to name a fragment.
 */
function fetchesFromCss(text: string): boolean {
  const decoded = decodeCssEscapes(text).toLowerCase();
  return CSS_FETCH.test(decoded) || (text.includes("\\") && CSS_URL.test(decoded));
}

function clean(parent: Element | DocumentFragment): void {
  for (const element of Array.from(parent.children)) {
    const name = element.localName.toLowerCase();
    if (BLOCKED_ELEMENTS.has(name)) {
      element.remove();
      continue;
    }
    for (const attribute of Array.from(element.attributes)) {
      const attr = attribute.name.toLowerCase();
      if (
        attr.startsWith("on") ||
        attr === "srcset" ||
        (URL_ATTRIBUTES.has(attr) && !SAFE_URL.test(attribute.value.trim())) ||
        fetchesFromCss(attribute.value)
      ) {
        element.removeAttribute(attribute.name);
      }
    }
    if (name === "style") {
      // The stylesheet is the element's own text nodes; with an element among
      // them, textContent is not that text.
      if (element.children.length > 0 || fetchesFromCss(element.textContent ?? "")) element.remove();
      continue;
    }
    clean(element);
  }
}

/**
 * Markup a renderer produced, parsed into an inert fragment with everything
 * that could run, load or navigate taken out. The editor's window can call
 * the app, so what goes into it is held to less than the preview's frame
 * allows: no script, no event handler, no link out of the drawing, and no
 * stylesheet or attribute whose CSS fetches anything.
 *
 * The fragment is what gets inserted, never markup serialized back out of it:
 * a second parse of the cleaned text could read it differently from the
 * first.
 */
export function sanitizedFragment(markup: string): DocumentFragment {
  const template = document.createElement("template");
  template.innerHTML = markup;
  clean(template.content);
  return template.content;
}
