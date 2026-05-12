import { javascript } from "@codemirror/lang-javascript";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { json } from "@codemirror/lang-json";
import { html } from "@codemirror/lang-html";
import { css } from "@codemirror/lang-css";
import { markdown } from "@codemirror/lang-markdown";
import { php } from "@codemirror/lang-php";
import { register } from "./language-registry";

export function registerBuiltinLanguages(): void {
  register("javascript", () => javascript({ jsx: true }));
  register("typescript", () => javascript({ jsx: true, typescript: true }));
  register("python", () => python());
  register("rust", () => rust());
  register("json", () => json());
  register("html", () => html());
  register("css", () => css());
  register("markdown", () => markdown());
  register("php", () => php());
}
