import { EditorView } from "@codemirror/view";
import { HighlightStyle } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import type { ThemePolarity } from "../../types/theme";
import "./cm-markdown-typography.css";

const SELECTION_ALPHA = "color-mix(in srgb, var(--writ-accent-default) 32%, transparent)";
const SELECTION_MATCH_ALPHA = "color-mix(in srgb, var(--writ-accent-default) 18%, transparent)";
const ACTIVE_LINE_ALPHA = "color-mix(in srgb, var(--writ-surface-hover) 55%, transparent)";

const writThemeSpec = {
    "&": {
      color: "var(--writ-foreground-default)",
      backgroundColor: "var(--writ-surface-background)",
      height: "100%",
      fontSize: "var(--writ-font-size)",
      fontFamily: "var(--writ-font-mono)",
    },
    ".cm-scroller": {
      overflow: "auto",
      fontFamily: "var(--writ-font-mono)",
    },
    ".cm-content": {
      padding: "8px 0",
      caretColor: "var(--writ-accent-default)",
    },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: "var(--writ-accent-default)",
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
      backgroundColor: "var(--writ-surface-background)",
      color: "var(--writ-foreground-subtle)",
      border: "none",
    },
    ".cm-activeLineGutter": {
      backgroundColor: "transparent",
      color: "var(--writ-foreground-muted)",
    },
    ".cm-lineNumbers .cm-gutterElement": {
      color: "var(--writ-foreground-subtle)",
      padding: "0 var(--writ-space-3) 0 var(--writ-space-3)",
    },
    ".cm-foldGutter .cm-gutterElement": {
      color: "var(--writ-foreground-subtle)",
    },
    ".cm-matchingBracket, .cm-nonmatchingBracket": {
      backgroundColor: SELECTION_MATCH_ALPHA,
      outline: "1px solid var(--writ-border-default)",
    },
    ".cm-searchMatch": {
      backgroundColor: SELECTION_MATCH_ALPHA,
      outline: "1px solid var(--writ-accent-default)",
    },
    ".cm-searchMatch.cm-searchMatch-selected": {
      backgroundColor: SELECTION_ALPHA,
    },
    ".cm-panels": {
      backgroundColor: "var(--writ-surface-raised)",
      color: "var(--writ-foreground-default)",
    },
    ".cm-panels.cm-panels-top": {
      borderBottom: "1px solid var(--writ-border-soft)",
    },
    ".cm-panels.cm-panels-bottom": {
      borderTop: "1px solid var(--writ-border-soft)",
    },
    ".cm-tooltip": {
      backgroundColor: "var(--writ-surface-elevated)",
      border: "1px solid var(--writ-border-soft)",
      color: "var(--writ-foreground-default)",
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

export const writHighlight = HighlightStyle.define([
  { tag: t.keyword, color: "var(--writ-syntax-keyword)" },
  { tag: t.controlKeyword, color: "var(--writ-syntax-keyword)" },
  { tag: t.moduleKeyword, color: "var(--writ-syntax-keyword)" },
  { tag: t.operatorKeyword, color: "var(--writ-syntax-keyword)" },
  { tag: t.definitionKeyword, color: "var(--writ-syntax-keyword)" },

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

  { tag: t.heading, color: "var(--writ-accent-default)", fontWeight: "600" },
  { tag: t.link, color: "var(--writ-accent-default)", textDecoration: "underline" },
  { tag: t.emphasis, fontStyle: "italic" },
  { tag: t.strong, fontWeight: "600" },

  { tag: t.invalid, color: "var(--writ-status-error)" },
  { tag: t.meta, color: "var(--writ-foreground-muted)" },
]);
