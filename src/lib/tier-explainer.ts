import type { FileOpenMode } from "../types/buffer";
import {
  featuresFor,
  hasOverrides,
  HIGHLIGHT_LINE_LIMIT,
  MAX_NORMAL_LINE_LENGTH,
  WRAP_LINE_LIMIT,
  type FileOverrides,
} from "../editor/large-file";
import { joinAnd } from "./join-and";

/** What the tier chip says about the file in front, and what it offers. */
export interface TierExplanation {
  /** The chip's own words. */
  chip: string;
  /** What is off and why, one sentence each. */
  notes: string[];
  /** The choices the person can make for this file. */
  toggles: { feature: keyof FileOverrides; label: string; on: boolean }[];
  /** A tier the document now calls for, as the action that moves it there. */
  offer: string | null;
}

/** The facts the chip is decided from. */
export interface TierFacts {
  mode: FileOpenMode | null;
  longest: number;
  overrides: FileOverrides;
  markdown: boolean;
  spellingOn: boolean;
  /** The tier the document calls for after a large edit, when it is another. */
  offered: FileOpenMode | null;
}

const formatCount = (n: number) => n.toLocaleString("en-US");

/** The tier's name where the status bar is not there to show it: the tab tooltip. */
export function tierName(mode: FileOpenMode | null): string | null {
  switch (mode?.kind) {
    case "LargeFile":
    case "LargeFileConfirm":
      return "Large file";
    case "LongLines":
      return "Long lines";
    case "Binary":
      return "Binary, read-only";
    default:
      return null;
  }
}

/**
 * What the chip says for a file, or null when it says nothing: a normal file
 * with nothing to offer.
 */
export function explainTier(facts: TierFacts): TierExplanation | null {
  const mode = facts.mode ?? { kind: "Normal" };
  const offered = facts.offered;
  if (mode.kind === "Binary") {
    return { chip: "Binary, read-only", notes: ["Shown as bytes. Nothing here can be edited."], toggles: [], offer: null };
  }
  if (mode.kind === "Normal") {
    if (offered === null) return null;
    return {
      chip: "Over 5 MB",
      notes: ["This file is now over 5 MB. Large-file mode keeps typing fast by turning formatting, spell check and links off."],
      toggles: [],
      offer: "Use large-file mode",
    };
  }

  const features = featuresFor(mode, facts.longest, facts.overrides);
  const defaults = featuresFor(mode, facts.longest);
  const notes: string[] = [];
  if (mode.kind === "LongLines") {
    notes.push(`A line is over ${formatCount(MAX_NORMAL_LINE_LENGTH)} characters.`);
    const perLine = [...(facts.markdown ? ["formatting"] : []), "links", "text direction"];
    notes.push(`Lines that long show without ${joinAnd(perLine)}.`);
    if (facts.spellingOn) notes.push("Spell check is off for this file.");
    if (!defaults.wrap || !defaults.highlight) {
      notes.push(
        `A line over ${formatCount(Math.min(WRAP_LINE_LIMIT, HIGHLIGHT_LINE_LIMIT))} characters starts with wrapping and highlighting off.`,
      );
    }
  } else {
    notes.push("Files over 5 MB open in large-file mode.");
    const off = [
      ...(facts.markdown ? ["Markdown formatting"] : []),
      ...(facts.spellingOn ? ["spell check"] : []),
      "links",
      "text direction",
    ];
    notes.push(`Off: ${joinAnd(off)}.`);
    notes.push("Not in search results.");
  }

  const toggles = hasOverrides(mode)
    ? [
        { feature: "wrap" as const, label: "Wrap lines", on: features.wrap },
        { feature: "highlight" as const, label: "Highlight syntax", on: features.highlight },
      ]
    : [];

  let offer: string | null = null;
  if (offered !== null && offered.kind !== "LargeFile" && offered.kind !== "LargeFileConfirm") {
    notes.push("This file is now under 5 MB.");
    offer = "Use the full editor";
  }

  return { chip: tierName(mode) ?? "", notes, toggles, offer };
}
