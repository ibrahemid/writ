// First-party Mermaid init, served same-origin under _assets so it runs under
// the document CSP's `script-src 'self'` whether or not the file is opted in
// to run its own scripts. `securityLevel:'strict'` sanitises diagram-authored
// HTML; `'sandbox'` is avoided because it wraps each diagram in an iframe,
// which the document CSP's `frame-src 'none'` forbids. The theme follows the
// document polarity so edge labels stay readable on a dark page.
window.mermaid.initialize({
  startOnLoad: false,
  securityLevel: "strict",
  theme:
    document.documentElement.getAttribute("data-writ-theme") === "dark"
      ? "dark"
      : "default",
});
window.mermaid.run({ querySelector: "pre.mermaid" });
