import { EditorState } from "@codemirror/state";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { syntaxTree } from "@codemirror/language";
const doc = "visit https://writ.dev today\n";
const state = EditorState.create({ doc, extensions: [markdown({ base: markdownLanguage })] });
syntaxTree(state).iterate({ enter: (n) => { console.log(n.name, n.from, n.to); } });
