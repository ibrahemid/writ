export type Platform = "mac" | "win" | "linux";

export function detectPlatform(): Platform {
  if (typeof navigator === "undefined") return "linux";
  const platform = navigator.platform.toLowerCase();
  if (/mac|iphone|ipad/.test(platform)) return "mac";
  if (/win/.test(platform)) return "win";
  return "linux";
}

const PLATFORMS: readonly Platform[] = ["mac", "win", "linux"];

/**
 * The platform the chrome renders as. `VITE_WRIT_PLATFORM` forces one in a dev
 * build so the Windows and GNOME shells can be driven and captured on a Mac; a
 * release build always reads the host.
 */
export function resolvePlatform(): Platform {
  if (import.meta.env.DEV) {
    const forced = import.meta.env.VITE_WRIT_PLATFORM;
    if (typeof forced === "string" && (PLATFORMS as readonly string[]).includes(forced)) {
      return forced as Platform;
    }
  }
  return detectPlatform();
}

export const IS_MAC = detectPlatform() === "mac";
