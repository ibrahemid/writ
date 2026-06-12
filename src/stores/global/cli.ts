import { installCli as ipcInstallCli } from "../../services/tauri";
import type { InstallCliResult } from "../../services/tauri";

export type { InstallCliResult };

export async function installCli(): Promise<InstallCliResult> {
  return ipcInstallCli();
}
