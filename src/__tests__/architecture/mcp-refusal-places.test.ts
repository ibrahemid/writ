import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { SECTION_LABELS, SETTINGS_INDEX } from "../../settings";
import { APPS } from "../../lib/apps";
import { APP_REGISTRATIONS } from "../commands/app-registrations";

const TOOLS_RS = readFileSync(resolve(process.cwd(), "crates/writ-mcp/src/tools.rs"), "utf8");

function refusalContaining(marker: string): string {
  const texts = [...TOOLS_RS.matchAll(/#\[error\(\s*"([^"]*)"/g)].map((match) => match[1]!);
  const found = texts.filter((text) => text.includes(marker));
  expect(found, `one refusal in crates/writ-mcp/src/tools.rs says "${marker}"`).toHaveLength(1);
  return found[0]!;
}

function placeAfter(text: string, lead: string): string[] {
  const at = text.indexOf(lead);
  expect(at, `"${text}" says "${lead}"`).toBeGreaterThan(-1);
  return text
    .slice(at + lead.length)
    .replace(/\.$/, "")
    .split(", ");
}

const SETTINGS = APP_REGISTRATIONS.get("settings.open")?.label;
const PROGRAMS = APPS.find((app) => app.id === "programs")?.label;
const WAITING = SETTINGS_INDEX.find((entry) => entry.id === "mcp.waiting");

describe("a refusal sends the person to a place Writ has", () => {
  it("reads the names it checks against", () => {
    expect(SETTINGS).toBe("Settings");
    expect(PROGRAMS).toBe("Connected programs");
    expect(WAITING?.section).toBe("apps");
    expect(WAITING?.requires).toEqual(["programs"]);
  });

  it("the waiting refusal names the list that holds the approve control", () => {
    const text = refusalContaining("is waiting for approval");
    expect(placeAfter(text, "in Writ, under ")).toEqual([
      SETTINGS,
      SECTION_LABELS.apps,
      PROGRAMS,
      WAITING?.title,
    ]);
  });

  it("the off refusal names the app and where its switch is", () => {
    const text = refusalContaining("is off in Writ");
    expect(text.startsWith(`${PROGRAMS} is off in Writ.`)).toBe(true);
    expect(placeAfter(text, "Turn it on in ")).toEqual([SETTINGS, SECTION_LABELS.apps]);
  });

  it("the not-allowed refusal names where the switches are", () => {
    const text = refusalContaining("is not allowed to");
    expect(placeAfter(text, "Allow it in ")).toEqual([SETTINGS, SECTION_LABELS.apps, PROGRAMS]);
  });
});
