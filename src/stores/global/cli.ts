import {
  cliStatus as ipcCliStatus,
  installCli as ipcInstallCli,
} from "../../services/tauri";
import type { CliStatus, InstallCliResult } from "../../services/tauri";

export type { CliStatus, InstallCliResult };

export async function fetchCliStatus(): Promise<CliStatus> {
  return ipcCliStatus();
}

export async function installCli(): Promise<InstallCliResult> {
  return ipcInstallCli();
}
