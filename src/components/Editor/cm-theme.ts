import { EditorView } from "@codemirror/view";
import { HighlightStyle } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import type { ThemePolarity } from "../../types/theme";
import { CSS_VAR, cssVar } from "../../styles/generated/tokens";
import "./cm-markdown-typography.css";

// Values are `var()` references, never baked colours: the theme switches live
// by rewriting custom properties on the root, and the site island renders this
// same spec against its own values (ADR-030).
// The app's own selection, so the note and the find field select in one colour;
// the match is the same mix a step weaker, and both are named in the theme.
const SELECTION = "var(--jotra-selection)";
const SELECTION_MATCH = "var(--jotra-selection-match)";
const ACTIVE_LINE_ALPHA = `color-mix(in srgb, ${cssVar(CSS_VAR.bgHover)} 55%, transparent)`;


// Exported for the contract test that holds the var()-only rule.
export const jotraThemeSpec = {
    "&": {
      color: cssVar(CSS_VAR.fg),
      backgroundColor: cssVar(CSS_VAR.bgCanvas),
      height: "100%",
      // The zoom lever, which defaults to the prose size (design/tokens).
      fontSize: "var(--jotra-editor-font-size)",
      lineHeight: cssVar(CSS_VAR.proseLineHeight),
      fontFamily: "var(--jotra-font-prose)",
      [CSS_VAR.prosePadX]: `calc(var(--jotra-editor-font-size) * ${cssVar(CSS_VAR.prosePadXEm)})`,
      [CSS_VAR.prosePadY]: `calc(var(--jotra-editor-font-size) * ${cssVar(CSS_VAR.prosePadYEm)})`,
    },
    ".cm-scroller": {
      overflow: "auto",
      minWidth: "0",
      lineHeight: cssVar(CSS_VAR.proseLineHeight),
      fontFamily: "var(--jotra-font-prose)",
    },
    // The column is the pane; only the gutters are drawn, in the same tokens
    // the rendered note pads with.
    ".cm-content": {
      maxWidth: "none",
      margin: "0 auto",
      padding: `var(${CSS_VAR.prosePadY}) var(${CSS_VAR.prosePadX})`,
      caretColor: cssVar(CSS_VAR.accent),
      WebkitUserSelect: "text",
      userSelect: "text",
    },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: cssVar(CSS_VAR.accent),
    },
    "&.cm-focused > .cm-scroller > .cm-jotra-selection .cm-selectionBackground": {
      backgroundColor: SELECTION,
    },
    ".cm-jotra-selection .cm-selectionBackground": {
      backgroundColor: SELECTION,
    },
    // The selection layer paints the note; a native selection reaching a
    // block widget would paint over it a second time.
    ".cm-content ::selection": {
      backgroundColor: "transparent",
    },
    ".cm-selectionMatch": {
      backgroundColor: SELECTION_MATCH,
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
      fontFamily: "var(--jotra-font-mono)",
      padding: "0 var(--jotra-space-3) 0 var(--jotra-space-3)",
    },
    ".cm-foldGutter .cm-gutterElement": {
      color: cssVar(CSS_VAR.fgFaint),
    },
    ".cm-fold-marker": {
      display: "inline-flex",
      alignItems: "center",
      height: "100%",
      padding: "0 var(--jotra-space-1)",
      cursor: "pointer",
    },
    ".cm-fold-marker:hover": {
      color: cssVar(CSS_VAR.fgMuted),
    },
    ".cm-fold-chevron": {
      width: "0.85em",
      height: "0.85em",
    },
    ".cm-fold-marker[data-folded] .cm-fold-chevron": {
      transform: "rotate(-90deg)",
    },
    ".cm-foldPlaceholder": {
      backgroundColor: cssVar(CSS_VAR.bgHover),
      border: "none",
      borderRadius: `var(${CSS_VAR.rControl})`,
      color: cssVar(CSS_VAR.fgMuted),
      margin: "0 0.25em",
      padding: "0 0.4em",
      cursor: "pointer",
    },
    ".cm-matchingBracket, .cm-nonmatchingBracket": {
      backgroundColor: SELECTION_MATCH,
      outline: `1px solid ${cssVar(CSS_VAR.border)}`,
    },
    ".cm-searchMatch": {
      backgroundColor: SELECTION_MATCH,
      outline: `1px solid ${cssVar(CSS_VAR.accent)}`,
    },
    ".cm-searchMatch.cm-searchMatch-selected": {
      backgroundColor: SELECTION,
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
export const jotraThemeDark = EditorView.theme(jotraThemeSpec, { dark: true });
export const jotraThemeLight = EditorView.theme(jotraThemeSpec, { dark: false });

export function editorThemeFor(polarity: ThemePolarity) {
  return polarity === "light" ? jotraThemeLight : jotraThemeDark;
}

/**
 * Mono for the whole surface. Applied to a buffer whose language is not
 * markdown: prose sans is the writing face, and a source file is code all the
 * way down. Inside a markdown buffer, mono stays scoped to `.cm-md-code` and
 * `.cm-md-codeblock` (ADR-030 decision 7).
 */
export const jotraCodeFace = EditorView.theme({
  "&, .cm-scroller, .cm-content": {
    fontFamily: "var(--jotra-font-mono)",
  },
});

/** The tag styles, exported so the same contract test can read them. */
export const JOTRA_HIGHLIGHT_SPECS = [
  { tag: t.keyword, color: "var(--jotra-syntax-keyword)", fontWeight: "600" },
  { tag: t.controlKeyword, color: "var(--jotra-syntax-keyword)", fontWeight: "600" },
  { tag: t.moduleKeyword, color: "var(--jotra-syntax-keyword)", fontWeight: "600" },
  { tag: t.operatorKeyword, color: "var(--jotra-syntax-keyword)", fontWeight: "600" },
  { tag: t.definitionKeyword, color: "var(--jotra-syntax-keyword)", fontWeight: "600" },

  { tag: [t.string, t.special(t.string)], color: "var(--jotra-syntax-string)" },
  { tag: t.regexp, color: "var(--jotra-syntax-string)" },
  { tag: t.escape, color: "var(--jotra-syntax-number)" },

  { tag: [t.comment, t.lineComment, t.blockComment, t.docComment], color: "var(--jotra-syntax-comment)", fontStyle: "italic" },

  { tag: [t.function(t.variableName), t.function(t.propertyName)], color: "var(--jotra-syntax-function)" },
  { tag: t.macroName, color: "var(--jotra-syntax-function)" },

  { tag: [t.number, t.integer, t.float, t.bool, t.null], color: "var(--jotra-syntax-number)" },

  { tag: [t.typeName, t.className, t.namespace], color: "var(--jotra-syntax-type)" },
  { tag: t.standard(t.typeName), color: "var(--jotra-syntax-type)" },

  { tag: [t.variableName, t.propertyName, t.attributeName], color: "var(--jotra-syntax-variable)" },
  { tag: t.definition(t.variableName), color: "var(--jotra-syntax-variable)" },

  { tag: [t.tagName, t.angleBracket], color: "var(--jotra-syntax-keyword)" },
  { tag: t.attributeValue, color: "var(--jotra-syntax-string)" },

  // Weight and size carry heading hierarchy; the ink is the body's (ADR-030
  // decision 3), and the per-level scale comes from the line decorations.
  { tag: t.heading, fontWeight: "600" },
  { tag: t.link, color: cssVar(CSS_VAR.accent), textDecoration: "underline" },
  { tag: t.emphasis, fontStyle: "italic" },
  { tag: t.strong, fontWeight: "600" },

  { tag: t.invalid, color: "var(--jotra-status-error)" },
  { tag: t.meta, color: cssVar(CSS_VAR.fgMuted) },
];

export const jotraHighlight = HighlightStyle.define(JOTRA_HIGHLIGHT_SPECS);
