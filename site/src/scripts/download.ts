import { detectOs, type NavigatorLike, type OsKey } from './platform';

/** How long a copy result stays on screen before the status line clears. */
export const COPY_STATUS_MS = 2400;

export interface DownloadEnv {
  nav: NavigatorLike;
  clipboard: Pick<Clipboard, 'writeText'> | undefined;
  /** Selects the command's text, so a failed copy leaves it ready for the keyboard. */
  selectText(node: Node): void;
  setTimeout(fn: () => void, ms: number): number;
  clearTimeout(id: number): void;
}

/** Names the copy keys of the visitor's own keyboard, whichever group's command they chose. */
export function getCopyFallbackMessage(visitor: OsKey | null): string {
  return visitor === 'mac' ? 'Selected, press ⌘C' : 'Selected, press Ctrl+C';
}

/**
 * Puts the visitor's operating system first, so the order people tab through
 * follows what they see, and arms each group's copy button. Without this the
 * groups stay in markup order (macOS, Windows, Linux), the copy buttons stay
 * hidden and every command is plain selectable text.
 */
export function startDownloadGroups(container: HTMLElement, env: DownloadEnv): void {
  const groups = [...container.querySelectorAll<HTMLElement>('[data-os]')];
  const visitor = detectOs(env.nav);
  const own = groups.find((group) => group.dataset.os === visitor);
  if (own && own !== container.firstElementChild) container.insertBefore(own, container.firstElementChild);

  for (const group of groups) {
    const button = group.querySelector<HTMLButtonElement>('[data-copy]');
    const code = group.querySelector<HTMLElement>('[data-copy-text]');
    const status = group.querySelector<HTMLElement>('[data-copy-status]');
    if (!button || !code || !status) continue;
    let clearTimer: number | null = null;

    const say = (message: string): void => {
      if (clearTimer !== null) env.clearTimeout(clearTimer);
      status.textContent = message;
      clearTimer = env.setTimeout(() => {
        status.textContent = '';
        clearTimer = null;
      }, COPY_STATUS_MS);
    };

    const selectInstead = (): void => {
      env.selectText(code);
      say(getCopyFallbackMessage(visitor));
    };

    const copy = async (): Promise<void> => {
      const text = (code.textContent ?? '').trim();
      status.textContent = '';
      if (!env.clipboard) {
        selectInstead();
        return;
      }
      try {
        await env.clipboard.writeText(text);
      } catch (err) {
        if (!(err instanceof DOMException)) throw err;
        selectInstead();
        return;
      }
      say('Copied');
    };

    button.addEventListener('click', () => {
      void copy();
    });
    button.hidden = false;
  }
}
