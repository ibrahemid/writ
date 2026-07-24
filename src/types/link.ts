// Mirrors `LinkVerdict` in src-tauri/src/commands/link.rs. `url` carries the
// normalized destination when the link is allowed; `reason` and `message`
// carry the refusal code and its user-facing text when it is not.
export interface LinkVerdict {
  allowed: boolean;
  url: string | null;
  reason: string | null;
  message: string | null;
}
