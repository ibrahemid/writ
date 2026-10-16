export class ChangelogImageMissingError extends Error {
  readonly file: string;

  constructor(file: string) {
    super(`changelog image ${file} is not in src/assets/changelog`);
    this.file = file;
    this.name = 'ChangelogImageMissingError';
  }
}

export function findChangelogImage<T>(images: Record<string, T>, file: string): T {
  const match = Object.entries(images).find(([path]) => path.endsWith(`/${file}`));
  if (!match) throw new ChangelogImageMissingError(file);
  return match[1];
}
