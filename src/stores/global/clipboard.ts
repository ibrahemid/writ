import { writeClipboardText } from "../../services/clipboard";

/**
 * The clipboard as a component reaches it.
 *
 * Components call stores and stores call services, so a surface with nothing
 * else to keep — the crash screen — still goes through this rather than
 * reaching into `services/clipboard` itself.
 */
export const clipboardStore = {
  copyText(text: string): Promise<void> {
    return writeClipboardText(text);
  },
};
