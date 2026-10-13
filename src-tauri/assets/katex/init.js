// First-party KaTeX init, served same-origin under _assets so it runs under
// the document CSP's `script-src 'self'` whether or not the file is opted in.
// Each `span.math` the Markdown renderer emitted carries raw LaTeX as its text
// content; `displayMode` follows the `math-display` class. `throwOnError:false`
// renders a malformed expression inline instead of throwing.
(function () {
  var ns = document.querySelectorAll("span.math");
  for (var i = 0; i < ns.length; i++) {
    var el = ns[i];
    try {
      window.katex.render(el.textContent, el, {
        displayMode: el.classList.contains("math-display"),
        throwOnError: false,
      });
    } catch (e) {}
  }
})();
