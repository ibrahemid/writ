export type Platform = "mac" | "win" | "linux";

export function detectPlatform(): Platform {
  if (typeof navigator === "undefined") return "linux";
  const platform = navigator.platform.toLowerCase();
  if (/mac|iphone|ipad/.test(platform)) return "mac";
  if (/win/.test(platform)) return "win";
  return "linux";
}

export const IS_MAC = detectPlatform() === "mac";

// What each platform calls its file manager, so a button names the app the
// user will see when it opens.
const FILE_MANAGER_LABELS: Record<Platform, string> = {
  mac: "Show in Finder",
  win: "Show in Explorer",
  linux: "Show in Files",
};

export const SHOW_IN_FILE_MANAGER = FILE_MANAGER_LABELS[detectPlatform()];
