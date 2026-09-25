# Fonts

Two families, the Latin subsets of Wix Madefor Display and Wix Madefor Text as
variable fonts (weight 400 to 800), under the SIL Open Font License 1.1. Each
licence text sits next to its file.

| File | Family | Licence | Upstream |
| --- | --- | --- | --- |
| `wix-madefor-display-latin-variable.woff2` | Wix Madefor Display | `OFL-wix-madefor-display.txt` | https://github.com/google/fonts/tree/main/ofl/wixmadefordisplay |
| `wix-madefor-text-latin-variable.woff2` | Wix Madefor Text | `OFL-wix-madefor-text.txt` | https://github.com/google/fonts/tree/main/ofl/wixmadefortext |

Weights in use, per `site/src/styles/site.css`:

| Element | Family | Weight |
| --- | --- | --- |
| Hero heading (`.hero-h1`) and its nouns | Display | 700 |
| Page and section headings (`.prose h1` to `h3`, `.download-page h1`, `.feature h2`, `.download h2`), OS names (`.dl-os`) | Display | 600 |
| Nav wordmark (`.nav-mark`) | Display | 600 |
| Body text, the Copy status (`.dl-status`), nav links | Text | 400 |
| Buttons (`.btn`, `.dl-copy`), tool labels (`.dl-tool-name`), table headers (`.prose th`) | Text | 500 |
| `<strong>` in the docs page's app list (browser default `bolder`) | Text | 700 |

`<code>` and `<kbd>` are set in the platform's own monospace stack and load no
file.
