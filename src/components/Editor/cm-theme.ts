import { EditorView } from "@codemirror/view";
import { HighlightStyle } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import type { ThemePolarity } from "../../types/theme";
import { CSS_VAR, cssVar } from "../../styles/generated/tokens";
import "./cm-markdown-typography.css";

// Values are `var()` references, never baked colours: the theme switches live
// by rewriting custom properties on the root, and the site island renders this
// same spec against its own values (ADR-030).
const SELECTION_ALPHA = `color-mix(in srgb, ${cssVar(CSS_VAR.accent)} 32%, transparent)`;
const SELECTION_MATCH_ALPHA = `color-mix(in srgb, ${cssVar(CSS_VAR.accent)} 18%, transparent)`;
const ACTIVE_LINE_ALPHA = `color-mix(in srgb, ${cssVar(CSS_VAR.bgHover)} 55%, transparent)`;


// Exported for the contract test that holds the var()-only rule.
export const writThemeSpec = {
    "&": {
      color: cssVar(CSS_VAR.fg),
      backgroundColor: cssVar(CSS_VAR.bgCanvas),
      height: "100%",
      // The zoom lever, which defaults to the prose size (design/tokens).
      fontSize: "var(--writ-editor-font-size)",
      lineHeight: cssVar(CSS_VAR.proseLineHeight),
      fontFamily: "var(--writ-font-prose)",
    },
    ".cm-scroller": {
      overflow: "auto",
      lineHeight: cssVar(CSS_VAR.proseLineHeight),
      fontFamily: "var(--writ-font-prose)",
    },
    // One reading column, centred, with the same measure and padding the
    // rendered note uses.
    ".cm-content": {
      maxWidth: `calc(${cssVar(CSS_VAR.proseMeasure)} + 2 * ${cssVar(CSS_VAR.prosePadX)})`,
      margin: "0 auto",
      padding: `var(${CSS_VAR.prosePadY}) var(${CSS_VAR.prosePadX})`,
      caretColor: cssVar(CSS_VAR.accent),
    },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: cssVar(CSS_VAR.accent),
    },
    "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground": {
      backgroundColor: SELECTION_ALPHA,
    },
    ".cm-selectionBackground, .cm-content ::selection": {
      backgroundColor: SELECTION_ALPHA,
    },
    ".cm-selectionMatch": {
      backgroundColor: SELECTION_MATCH_ALPHA,
    },
    ".cm-activeLine": {
      backgroundColor: ACTIVE_LINE_ALPHA,
    },
    ".cm-gutters": {
      backgroundColor: cssVar(CSS_VAR.bgCanvas),
      color: cssVar(CSS_VAR.fgFaint),
      border: "none",
    },
    ".cm-activeLineGutter": {
      backgroundColor: "transparent",
      color: cssVar(CSS_VAR.fgMuted),
    },
    ".cm-lineNumbers .cm-gutterElement": {
      color: cssVar(CSS_VAR.fgFaint),
      fontFamily: "var(--writ-font-mono)",
      padding: "0 var(--writ-space-3) 0 var(--writ-space-3)",
    },
    ".cm-foldGutter .cm-gutterElement": {
      color: cssVar(CSS_VAR.fgFaint),
    },
    ".cm-matchingBracket, .cm-nonmatchingBracket": {
      backgroundColor: SELECTION_MATCH_ALPHA,
      outline: `1px solid ${cssVar(CSS_VAR.border)}`,
    },
    ".cm-searchMatch": {
      backgroundColor: SELECTION_MATCH_ALPHA,
      outline: `1px solid ${cssVar(CSS_VAR.accent)}`,
    },
    ".cm-searchMatch.cm-searchMatch-selected": {
      backgroundColor: SELECTION_ALPHA,
    },
    ".cm-panels": {
      backgroundColor: cssVar(CSS_VAR.bgRaised),
      color: cssVar(CSS_VAR.fg),
    },
    ".cm-panels.cm-panels-top": {
      borderBottom: `1px solid ${cssVar(CSS_VAR.borderSoft)}`,
    },
    ".cm-panels.cm-panels-bottom": {
      borderTop: `1px solid ${cssVar(CSS_VAR.borderSoft)}`,
    },
    ".cm-tooltip": {
      backgroundColor: cssVar(CSS_VAR.bgRaised),
      border: `1px solid ${cssVar(CSS_VAR.borderSoft)}`,
      color: cssVar(CSS_VAR.fg),
    },
};

// One spec, two polarities. The token values flip via CSS custom properties;
// the { dark } flag flips CodeMirror's own light/dark fallback styling so a
// light preset doesn't keep dark-mode caret/selection defaults.
export const writThemeDark = EditorView.theme(writThemeSpec, { dark: true });
export const writThemeLight = EditorView.theme(writThemeSpec, { dark: false });

export function editorThemeFor(polarity: ThemePolarity) {
  return polarity === "light" ? writThemeLight : writThemeDark;
}

/**
 * Mono for the whole surface. Applied to a buffer whose language is not
 * markdown: prose sans is the writing face, and a source file is code all the
 * way down. Inside a markdown buffer, mono stays scoped to `.cm-md-code` and
 * `.cm-md-codeblock` (ADR-030 decision 7).
 */
export const writCodeFace = EditorView.theme({
  "&, .cm-scroller, .cm-content": {
    fontFamily: "var(--writ-font-mono)",
  },
});

/** The tag styles, exported so the same contract test can read them. */
export const WRIT_HIGHLIGHT_SPECS = [
  { tag: t.keyword, color: "var(--writ-syntax-keyword)", fontWeight: "600" },
  { tag: t.controlKeyword, color: "var(--writ-syntax-keyword)", fontWeight: "600" },
  { tag: t.moduleKeyword, color: "var(--writ-syntax-keyword)", fontWeight: "600" },
  { tag: t.operatorKeyword, color: "var(--writ-syntax-keyword)", fontWeight: "600" },
  { tag: t.definitionKeyword, color: "var(--writ-syntax-keyword)", fontWeight: "600" },

  { tag: [t.string, t.special(t.string)], color: "var(--writ-syntax-string)" },
  { tag: t.regexp, color: "var(--writ-syntax-string)" },
  { tag: t.escape, color: "var(--writ-syntax-number)" },

  { tag: [t.comment, t.lineComment, t.blockComment, t.docComment], color: "var(--writ-syntax-comment)", fontStyle: "italic" },

  { tag: [t.function(t.variableName), t.function(t.propertyName)], color: "var(--writ-syntax-function)" },
  { tag: t.macroName, color: "var(--writ-syntax-function)" },

  { tag: [t.number, t.integer, t.float, t.bool, t.null], color: "var(--writ-syntax-number)" },

  { tag: [t.typeName, t.className, t.namespace], color: "var(--writ-syntax-type)" },
  { tag: t.standard(t.typeName), color: "var(--writ-syntax-type)" },

  { tag: [t.variableName, t.propertyName, t.attributeName], color: "var(--writ-syntax-variable)" },
  { tag: t.definition(t.variableName), color: "var(--writ-syntax-variable)" },

  { tag: [t.tagName, t.angleBracket], color: "var(--writ-syntax-keyword)" },
  { tag: t.attributeValue, color: "var(--writ-syntax-string)" },

  // Weight and size carry heading hierarchy; the ink is the body's (ADR-030
  // decision 3), and the per-level scale comes from the line decorations.
  { tag: t.heading, fontWeight: "600" },
  { tag: t.link, color: cssVar(CSS_VAR.accent), textDecoration: "underline" },
  { tag: t.emphasis, fontStyle: "italic" },
  { tag: t.strong, fontWeight: "600" },

  { tag: t.invalid, color: "var(--writ-status-error)" },
  { tag: t.meta, color: cssVar(CSS_VAR.fgMuted) },
];

export const writHighlight = HighlightStyle.define(WRIT_HIGHLIGHT_SPECS);
