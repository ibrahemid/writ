import type { NoteVersion } from "../../src/services/tauri";
import type { SaveState } from "../../src/stores/global/save-status";
import type { AppId } from "../../src/types/config";

export const SCENE_NAMES = ["hero", "any-file", "markdown", "search", "apps", "graph", "versions"] as const;

export type SceneName = (typeof SCENE_NAMES)[number];

export type SceneState = "done" | "cancelled";

export type SceneLayout = "inline" | "source";

export interface SceneEditor {
  readonly bufferId: string;
  readonly path: string;
  text(): string;
  replaceText(text: string): void;
  showFromTop(): void;
  placeCursorAtLineStart(line: number): void;
  placeCursorAtLineEnd(line: number): void;
  placeCursorAtEnd(): void;
  insert(text: string): void;
  setTyping(isTyping: boolean): void;
}

export interface ScenePalette {
  open(): void;
  close(): void;
  isReady(): boolean;
  setQuery(query: string): void;
}

export interface SceneVersions {
  open(path: string): void;
  close(): void;
  isLoaded(path: string): boolean;
  list(): readonly NoteVersion[];
  reset(note: string): Promise<void>;
  select(versionId: number): Promise<void>;
  restore(versionId: number): Promise<void>;
}

export interface SceneApp {
  seedText(note: string): string;
  openNote(note: string, signal: AbortSignal): Promise<SceneEditor>;
  openNoteAtLine(note: string, line: number, signal: AbortSignal): Promise<SceneEditor>;
  closeOtherTabs(editor: SceneEditor): Promise<void>;
  setLayout(editor: SceneEditor, layout: SceneLayout): void;
  restoreLayouts(): void;
  clearTyping(): void;
  saveState(editor: SceneEditor): SaveState;
  settings: { open(section: "apps"): void; close(): void };
  apps: { setOn(app: AppId, isOn: boolean): Promise<void> };
  graph: { open(): void; close(): void; isDrawn(): boolean; setQuery(query: string): void };
  palette: ScenePalette;
  versions: SceneVersions;
}

export type Scene = (app: SceneApp, signal: AbortSignal) => Promise<void>;

export type SceneSettle = (app: SceneApp, signal: AbortSignal, next: SceneName) => Promise<void>;

export interface ScenePlayer {
  play(name: SceneName): Promise<void>;
  cancel(): void;
}
