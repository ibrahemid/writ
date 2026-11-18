import {
  cliStatus as ipcCliStatus,
  installCli as ipcInstallCli,
  writeClipboardText,
} from "../../services/tauri";
import type { CliStatus, InstallCliResult } from "../../services/tauri";

export type { CliStatus, InstallCliResult };

/** The two commands the installed row offers to copy. */
export const CLI_EXAMPLES: readonly string[] = ["jotra .", "git log | jotra --title log"];

const INSTALL_FAILED = "Could not install the jotra command";

export async function fetchCliStatus(): Promise<CliStatus> {
  return ipcCliStatus();
}

export async function installCli(): Promise<InstallCliResult> {
  return ipcInstallCli();
}

/** What a failed install tells the person: the app's own sentence when it sent one. */
export function installFailureMessage(error: unknown): string {
  return typeof error === "string" && error !== "" ? error : INSTALL_FAILED;
}

export async function copyCliExample(example: string): Promise<void> {
  return writeClipboardText(example);
}
