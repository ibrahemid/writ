import * as tauri from "../../services/tauri";
import { logFailure } from "../../lib/log";
import type {
  ClipboardPasteResult,
  DropPoint,
  DroppedImagesResult,
} from "../../types/image-import";
import { showToast } from "./notifications";

/**
 * An editor that can take images dropped on it. Each pane's editor registers
 * one while it is mounted.
 */
export interface ImageDropTarget {
  /** The tab the editor holds now. */
  bufferId(): string;
  /** Whether that tab is a Markdown file that can be edited. */
  acceptsImages(): boolean;
  /** Whether `point` falls on the editor. */
  contains(point: DropPoint): boolean;
  hasFocus(): boolean;
  /**
   * Inserts `markdown` at `point`, or at the cursor when there is none, into
   * the tab `bufferId`. Answers false when the editor no longer holds it.
   */
  insert(markdown: string, point: DropPoint | null, bufferId: string): boolean;
}

export const DROP_FAILED_NOTICE = "Could not add the images";
export const DROP_TAB_CHANGED_NOTICE = "The tab changed before the images were added";
const NOTICE_MS = 6000;

// Singleton state — Writ is single-window
const targets = new Set<ImageDropTarget>();

/** Adds `target` to the editors a drop can land on; returns its removal. */
export function registerImageDropTarget(target: ImageDropTarget): () => void {
  targets.add(target);
  return () => {
    targets.delete(target);
  };
}

/**
 * The editor a drop at `point` lands on, when it takes images: the one under
 * the point, or the focused one when the platform gave no point.
 */
export function imageDropTargetFor(point: DropPoint | null): ImageDropTarget | null {
  for (const target of targets) {
    const hit = point ? target.contains(point) : target.hasFocus();
    if (hit) return target.acceptsImages() ? target : null;
  }
  return null;
}

/** Shows one message per image that was not taken. */
export function showImageNotices(notices: readonly string[]): void {
  for (const notice of notices) showToast(notice, "warning", NOTICE_MS);
}

/**
 * Saves the images among dropped `paths` into the Markdown file under the
 * drop and inserts their references there. Answers the paths left to open
 * as tabs: every path when the drop did not land on a Markdown editor.
 */
export async function takeDroppedImages(
  paths: string[],
  point: DropPoint | null,
): Promise<string[]> {
  if (!Array.isArray(paths) || paths.length === 0) return paths;
  const target = imageDropTargetFor(point);
  if (!target) return paths;
  const bufferId = target.bufferId();
  let result: DroppedImagesResult;
  try {
    result = await tauri.importDroppedImages(bufferId, paths);
  } catch {
    logFailure("dropped images could not be added to the file");
    showToast(DROP_FAILED_NOTICE, "error");
    return paths;
  }
  if (result.markdown && !target.insert(result.markdown, point, bufferId)) {
    showToast(DROP_TAB_CHANGED_NOTICE, "warning", NOTICE_MS);
  }
  showImageNotices(result.notices);
  return result.others;
}

/** Asks the host to paste the clipboard's images into the tab `bufferId`. */
export async function pasteClipboardImages(bufferId: string): Promise<ClipboardPasteResult> {
  return tauri.pasteClipboardImages(bufferId);
}

/** Fetches a web image an open Markdown tab references, on request. */
export async function loadRemoteImage(
  bufferId: string,
  reference: string,
): Promise<{ src: string }> {
  return tauri.previewRemoteImage(bufferId, reference);
}

export const imagesStore = {
  registerImageDropTarget,
  imageDropTargetFor,
  takeDroppedImages,
  pasteClipboardImages,
  loadRemoteImage,
  showImageNotices,
};
