/** Who left a text in a file, as Rust's `jotra_core::notes::Writer` sends it. */
export type VersionWriter =
  | { kind: "jotra" }
  | { kind: "program"; name: string }
  | { kind: "chat" }
  | { kind: "command" };
