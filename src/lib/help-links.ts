import type { Platform } from "./platform";

/** The site's documentation, which Help, Writ Help opens. */
export const DOCS_URL = "https://writ.ibrahemid.com/docs/";

const NEW_ISSUE_URL = "https://github.com/ibrahemid/writ/issues/new";

// The bug form's Operating System options (.github/ISSUE_TEMPLATE/bug_report.yml).
// Linux has two there, X11 and Wayland, which the platform alone cannot tell.
const OS_OPTION: Partial<Record<Platform, string>> = {
  mac: "macOS",
  win: "Windows",
};

/** The bug form, with the version and the system filled in where they are known. */
export function reportIssueUrl(version: string, platform: Platform): string {
  const params = new URLSearchParams({ template: "bug_report.yml", version });
  const os = OS_OPTION[platform];
  if (os) params.set("os", os);
  return `${NEW_ISSUE_URL}?${params.toString()}`;
}
