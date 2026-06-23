export type PlatformKey = 'mac' | 'win' | 'linux' | null;

export function detectPlatform(): PlatformKey {
  try {
    // Prefer modern UA hints (available in Chromium 90+)
    const uap = (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData;
    if (uap?.platform) {
      const p = uap.platform.toLowerCase();
      if (p === 'macos' || p === 'mac os x') return 'mac';
      if (p === 'windows') return 'win';
      if (p === 'linux') return 'linux';
    }
    // Legacy fallback
    const plat = navigator.platform?.toLowerCase() ?? '';
    if (plat.startsWith('mac')) return 'mac';
    if (plat.startsWith('win')) return 'win';
    if (plat.startsWith('linux') || plat.startsWith('freebsd')) return 'linux';
  } catch {
    // detection unavailable (SSR, privacy override, etc.)
  }
  return null;
}

export function promoteCard(doc: Document, platform: PlatformKey): void {
  if (!platform) return;
  const grid = doc.getElementById('dlGrid');
  if (!grid) return;
  const card = grid.querySelector<HTMLElement>(`[data-platform="${platform}"]`);
  if (!card) return;
  card.classList.add('dl-lead');
}
