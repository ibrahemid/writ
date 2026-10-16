import type { ChangelogPlatform } from '../data/changelog';

export const CHANGELOG_PLATFORM_LABELS: Readonly<Record<ChangelogPlatform, string>> = {
  macos: 'macOS',
  windows: 'Windows',
  linux: 'Linux',
};

const PLATFORM_LEAD = /^on (macOS|Windows|Linux)(?: and (macOS|Windows|Linux))?,? (?!\d)/;

export function stripPlatformLead(text: string, platforms: readonly ChangelogPlatform[] | undefined): string {
  if (!platforms || platforms.length === 0) return text;
  const lead = PLATFORM_LEAD.exec(text);
  if (!lead) return text;
  const named = new Set(lead.slice(1).filter((name): name is string => name !== undefined));
  const tagged = new Set(platforms.map((platform) => CHANGELOG_PLATFORM_LABELS[platform]));
  if (named.size !== tagged.size || [...named].some((name) => !tagged.has(name))) return text;
  return text.slice(lead[0].length);
}
