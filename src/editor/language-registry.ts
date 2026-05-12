import type { Extension } from "@codemirror/state";

export type LanguageFactory = () => Extension;

interface LanguageEntry {
  id: string;
  factory: LanguageFactory;
}

const entries = new Map<string, LanguageEntry>();

export function register(id: string, factory: LanguageFactory): void {
  entries.set(id, { id, factory });
}

export function getExtension(id: string | null): Extension {
  if (id === null) return [];
  const entry = entries.get(id);
  if (!entry) return [];
  return entry.factory();
}

export function listLanguageIds(): string[] {
  return Array.from(entries.keys());
}

export function unregisterAll(): void {
  entries.clear();
}
