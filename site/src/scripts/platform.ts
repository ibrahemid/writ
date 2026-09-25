export const OS_KEYS = ['mac', 'win', 'linux'] as const;

export type OsKey = (typeof OS_KEYS)[number];

export interface NavigatorLike {
  platform?: string;
  userAgent?: string;
  userAgentData?: { platform?: string } | null;
}

/**
 * The visitor's operating system, read the way the hero's download button in
 * index.astro reads it, or null when the platform names none of the three.
 * Android reports a Linux platform and gets null, since no build runs there.
 */
export function detectOs(nav: NavigatorLike): OsKey | null {
  const platform = nav.userAgentData?.platform || nav.platform || '';
  const ua = nav.userAgent || '';
  if (/win/i.test(platform)) return 'win';
  if (/linux/i.test(platform) && !/android/i.test(ua)) return 'linux';
  if (/mac/i.test(platform)) return 'mac';
  return null;
}
