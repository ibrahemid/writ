import type { VersionWriter } from "../types/writer";
import { displayProgramName } from "./program-names";

export type { VersionWriter };

/** Reads a writer off the wire, or `null` for anything that is not one. */
export function readWriter(value: unknown): VersionWriter | null {
  if (typeof value !== "object" || value === null) return null;
  const kind = (value as { kind?: unknown }).kind;
  if (kind === "writ" || kind === "chat" || kind === "command") return { kind };
  if (kind === "program") {
    const name = (value as { name?: unknown }).name;
    if (typeof name === "string" && name.trim() !== "") return { kind, name };
  }
  return null;
}

/** The name a person reads for a writer. */
export function writerName(writer: VersionWriter): string {
  switch (writer.kind) {
    case "writ":
      return "Writ";
    case "program":
      return displayProgramName(writer.name);
    case "chat":
      return "Chat";
    case "command":
      return "writ command";
  }
}

/** The program a writer names, by the name a person reads, or `null`. */
export function programName(writer: VersionWriter | null | undefined): string | null {
  return writer?.kind === "program" ? displayProgramName(writer.name) : null;
}
