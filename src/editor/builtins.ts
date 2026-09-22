import { javascript } from "@codemirror/lang-javascript";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { json } from "@codemirror/lang-json";
import { html } from "@codemirror/lang-html";
import { css } from "@codemirror/lang-css";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { languages } from "@codemirror/language-data";
import { php } from "@codemirror/lang-php";
import { sql } from "@codemirror/lang-sql";
import { register } from "./language-registry";

export function registerBuiltinLanguages(): void {
  register("javascript", () => javascript({ jsx: true }));
  register("typescript", () => javascript({ jsx: true, typescript: true }));
  register("python", () => python());
  register("rust", () => rust());
  register("json", () => json());
  register("html", () => html());
  register("css", () => css());
  // The code-language table nests an existing CodeMirror parser under the
  // one markdown tree, so a fenced block is highlighted in its own language.
  register("markdown", () => markdown({ base: markdownLanguage, codeLanguages: languages }));
  register("php", () => php());
  register("sql", () => sql());
}
