export type Platform = "mac" | "win" | "linux";

export function detectPlatform(): Platform {
  if (typeof navigator === "undefined") return "linux";
  const platform = navigator.platform.toLowerCase();
  if (/mac|iphone|ipad/.test(platform)) return "mac";
  if (/win/.test(platform)) return "win";
  return "linux";
}

export const IS_MAC = detectPlatform() === "mac";
