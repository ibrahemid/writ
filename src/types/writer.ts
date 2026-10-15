/** Who left a text in a file, as Rust's `writ_core::notes::Writer` sends it. */
export type VersionWriter =
  | { kind: "writ" }
  | { kind: "program"; name: string }
  | { kind: "chat" }
  | { kind: "command" };
