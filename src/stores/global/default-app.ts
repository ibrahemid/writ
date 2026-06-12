import {
  getDefaultAppStatus as ipcGetDefaultAppStatus,
  setDefaultApp as ipcSetDefaultApp,
} from "../../services/tauri";
import type { DefaultAppStatus } from "../../services/tauri";

export type { DefaultAppStatus };

export async function fetchDefaultAppStatus(ext: string): Promise<DefaultAppStatus> {
  return ipcGetDefaultAppStatus(ext);
}

export async function claimDefaultApp(ext: string): Promise<void> {
  return ipcSetDefaultApp(ext);
}
