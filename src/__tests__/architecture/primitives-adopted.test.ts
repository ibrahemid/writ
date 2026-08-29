import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// Surfaces that have moved onto the primitives. An inline <svg> is a second
// icon set, and a title= attribute is a second tooltip, so neither may come
// back to a migrated file. Later units add their own files here.

const REPO_ROOT = process.cwd();

const MIGRATED = [
  "src/components/Button/Button.tsx",
  "src/components/Kbd/Kbd.tsx",
  "src/components/Sidebar/SearchBar.tsx",
  "src/components/Tooltip/Tooltip.tsx",
];

const ICON_OWNERS = ["src/components/Icon/Icon.tsx", "src/components/Icon/IconSprite.tsx"];

function read(rel: string): string {
  return readFileSync(resolve(REPO_ROOT, rel), "utf8");
}

describe("migrated surfaces use the primitives", () => {
  it("no inline svg icon remains", () => {
    const offenders = MIGRATED.filter((rel) => read(rel).includes("<svg"));
    expect(offenders, `inline <svg> found in: ${offenders.join(", ")}`).toEqual([]);
  });

  it("no title attribute stands in for a tooltip", () => {
    const offenders = [...MIGRATED, ...ICON_OWNERS].filter((rel) => /\btitle=/.test(read(rel)));
    expect(offenders, `title= found in: ${offenders.join(", ")}`).toEqual([]);
  });

  it("the sprite is the only place svg markup is authored", () => {
    expect(read("src/components/Icon/Icon.tsx")).toContain("<use href=");
    expect(read("src/components/Sidebar/SearchBar.tsx")).toContain('<Icon name="magnifying-glass"');
  });
});
