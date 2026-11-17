import { defineAppCommands } from "./app-commands";
import type { DictationStore } from "../stores/window/dictation-store";

/** Registers the Dictation command, which follows the Dictation switch like
 * every app's (ADR-042 section 3). */
export function defineDictationCommands(dictation: Pick<DictationStore, "toggle">): void {
  defineAppCommands("dictation", [
    {
      id: "dictation.toggle",
      icon: "microphone",
      label: "Dictate",
      description: "Insert speech as text at the cursor",
      keywords: ["dictate", "voice", "speech", "microphone", "transcribe"],
      keybinding: "CmdOrCtrl+Shift+.",
      scope: "app",
      // Global, like the chat toggle: the editor holds focus while you write.
      global: true,
      execute: () => dictation.toggle(),
    },
  ]);
}
