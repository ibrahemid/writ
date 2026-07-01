import {
  getStorageInfo as ipcGetStorageInfo,
  revealStoragePath as ipcRevealStoragePath,
} from "../../services/tauri";
import type { StorageInfo } from "../../services/tauri";

export type { StorageInfo };

export async function fetchStorageInfo(): Promise<StorageInfo> {
  return ipcGetStorageInfo();
}

export async function revealStoragePath(): Promise<void> {
  return ipcRevealStoragePath();
}
