// Tauri rejects IPC with a plain string; a thrown Error carries its message.
export function formatSaveError(error: unknown): string {
  const text = error instanceof Error ? error.message : String(error ?? "");
  const trimmed = text.trim();
  return trimmed.length > 0 ? trimmed : "unknown error";
}
