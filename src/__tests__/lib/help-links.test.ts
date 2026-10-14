import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { DOCS_URL, reportIssueUrl } from "../../lib/help-links";

const BUG_FORM = readFileSync(resolve(process.cwd(), ".github/ISSUE_TEMPLATE/bug_report.yml"), "utf8");

describe("Help's links", () => {
  it("opens the site's docs page", () => {
    expect(DOCS_URL).toBe("https://writ.ibrahemid.com/docs/");
  });

  it("opens the bug form with the version and the system filled in", () => {
    const url = new URL(reportIssueUrl("0.7.0", "mac"));
    expect(`${url.origin}${url.pathname}`).toBe("https://github.com/ibrahemid/writ/issues/new");
    expect(url.searchParams.get("template")).toBe("bug_report.yml");
    expect(url.searchParams.get("version")).toBe("0.7.0");
    expect(url.searchParams.get("os")).toBe("macOS");
    expect(new URL(reportIssueUrl("0.7.0", "win")).searchParams.get("os")).toBe("Windows");
  });

  it("leaves the system to the reporter on Linux, where the form asks X11 or Wayland", () => {
    expect(new URL(reportIssueUrl("0.7.0", "linux")).searchParams.has("os")).toBe(false);
  });

  it("names fields and options the form has", () => {
    expect(BUG_FORM).toContain("id: version");
    expect(BUG_FORM).toContain("id: os");
    expect(BUG_FORM).toContain("- macOS");
    expect(BUG_FORM).toContain("- Windows");
  });
});
