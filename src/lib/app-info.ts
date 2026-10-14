import { version } from "../../package.json";
import { resolvePlatform } from "./platform";

/** The version the build was cut from. */
export const APP_VERSION: string = version;

/**
 * The details a person copies from a crash or boot error screen: the version,
 * the platform and the error, so a report says which build failed where.
 */
export function failureDetails(error: string): string {
  const agent = typeof navigator === "undefined" ? "" : ` (${navigator.userAgent})`;
  return [`Writ ${APP_VERSION}`, `Platform: ${resolvePlatform()}${agent}`, "", error].join("\n");
}
