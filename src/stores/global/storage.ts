import {
  getStorageInfo as ipcGetStorageInfo,
  revealStoragePath as ipcRevealStoragePath,
} from "../../services/tauri";
import type { StorageInfo } from "../../services/tauri";
import { writeClipboardText } from "../../services/clipboard";

export type { StorageInfo };

export async function fetchStorageInfo(): Promise<StorageInfo> {
  return ipcGetStorageInfo();
}

export async function revealStoragePath(): Promise<void> {
  return ipcRevealStoragePath();
}

export async function copyStoragePath(path: string): Promise<void> {
  return writeClipboardText(path);
}
