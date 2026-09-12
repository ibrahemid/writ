# Site design system

The site takes its colour, spacing, radius, motion and type tokens from
`design/tokens/` at the repository root. `pnpm tokens:build` writes them to
`site/src/styles/tokens.css`; nothing under `site/` declares a colour or a
length of its own. `site/src/styles/site.css` holds the rules, and the only
literal lengths in it are the three widths of the breakpoint set, which a
media query cannot read from a custom property.

`fonts/` holds the one self-hosted family, Inter. `site/design/REFERENCE.md`
records the reference set the layout was measured against and the lock taken
from it.
