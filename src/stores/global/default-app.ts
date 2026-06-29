import {
  getDefaultAppStatus as ipcGetDefaultAppStatus,
  listDefaultAppTypes as ipcListDefaultAppTypes,
  setDefaultApp as ipcSetDefaultApp,
} from "../../services/tauri";
import type { ClaimableType, DefaultAppStatus } from "../../services/tauri";

export type { ClaimableType, DefaultAppStatus };

export async function fetchDefaultAppTypes(): Promise<ClaimableType[]> {
  return ipcListDefaultAppTypes();
}

export async function fetchDefaultAppStatus(id: string): Promise<DefaultAppStatus> {
  return ipcGetDefaultAppStatus(id);
}

export async function claimDefaultApp(id: string): Promise<void> {
  return ipcSetDefaultApp(id);
}
