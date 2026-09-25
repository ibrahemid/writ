import { describe, it, expect } from 'vitest';
import { startLoopPlayback, type LoopEnv } from '../scripts/loops';

class FakeClassList {
  readonly names = new Set<string>();
  add(name: string): void {
    this.names.add(name);
  }
  remove(name: string): void {
    this.names.delete(name);
  }
  contains(name: string): boolean {
    return this.names.has(name);
  }
}

class FakeSource {
  readonly dataset: Record<string, string | undefined> = {};
  private readonly attributes = new Map<string, string>();
  constructor(src: string) {
    this.attributes.set('src', src);
  }
  getAttribute(name: string): string | null {
    return this.attributes.get(name) ?? null;
  }
  setAttribute(name: string, value: string): void {
    this.attributes.set(name, value);
  }
  removeAttribute(name: string): void {
    this.attributes.delete(name);
  }
}

class FakeVideo {
  paused = true;
  loads = 0;
  readonly poster: string;
  readonly sources: FakeSource[];
  readonly parentElement = { style: { setProperty: (_: string, __: string) => {} }, dataset: {} as Record<string, string>, classList: new FakeClassList() };
  private readonly listeners: Record<string, ((event: { key?: string; preventDefault(): void }) => void)[]> = {};
  constructor(name: string) {
    this.poster = `/_astro/${name}.webp`;
    this.sources = [new FakeSource(`/media/${name}-light.webm`), new FakeSource(`/media/${name}-light.mp4`)];
  }
  querySelectorAll(selector: string): FakeSource[] {
    return selector === 'source' ? this.sources : [];
  }
  addEventListener(type: string, fn: (event: { key?: string; preventDefault(): void }) => void): void {
    (this.listeners[type] ??= []).push(fn);
  }
  dispatch(type: string, key?: string): void {
    for (const fn of this.listeners[type] ?? []) fn({ key, preventDefault() {} });
  }
  pause(): void {
    this.paused = true;
  }
  play(): Promise<void> {
    this.paused = false;
    this.dispatch('playing');
    return Promise.resolve();
  }
  load(): void {
    this.loads += 1;
    this.paused = true;
  }
  srcs(): (string | null)[] {
    return this.sources.map((source) => source.getAttribute('src'));
  }
}

function setup(options: { reduced?: boolean } = {}) {
  const videos = [new FakeVideo('any-file'), new FakeVideo('search')];
  const reduced = { matches: Boolean(options.reduced), change: [] as (() => void)[] };
  const observers: {
    callback: (entries: { target: unknown; isIntersecting: boolean }[]) => void;
    observed: unknown[];
    disconnected: boolean;
  }[] = [];
  const env = {
    videos,
    reducedQuery: {
      get matches() {
        return reduced.matches;
      },
      addEventListener: (_: string, fn: () => void) => reduced.change.push(fn),
    },
    IntersectionObserver: class {
      readonly record;
      constructor(callback: (entries: { target: unknown; isIntersecting: boolean }[]) => void) {
        this.record = { callback, observed: [] as unknown[], disconnected: false };
        observers.push(this.record);
      }
      observe(target: unknown) {
        this.record.disconnected = false;
        this.record.observed.push(target);
      }
      disconnect() {
        this.record.disconnected = true;
        this.record.observed = [];
      }
    },
  };
  startLoopPlayback(env as unknown as LoopEnv);
  const watcher = observers[0];
  if (!watcher) throw new Error('startLoopPlayback made no IntersectionObserver');
  const showInView = (inView: FakeVideo[]): void => {
    if (watcher.disconnected) return;
    watcher.callback(watcher.observed.map((target) => ({ target, isIntersecting: inView.includes(target as FakeVideo) })));
  };
  const setReduced = (matches: boolean): void => {
    reduced.matches = matches;
    for (const fn of reduced.change) fn();
  };
  return { videos, watcher, showInView, setReduced };
}

describe('startLoopPlayback', () => {
  it('plays a loop in view over its poster and pauses it when it leaves', () => {
    const { videos, showInView } = setup();
    const [first, second] = videos;
    showInView([first!]);
    expect(first!.paused).toBe(false);
    expect(first!.loads).toBe(1);
    expect(first!.parentElement.dataset.fade).toBe('');
    expect(first!.parentElement.classList.contains('is-playing')).toBe(true);
    expect(second!.paused).toBe(true);
    showInView([second!]);
    expect(first!.paused).toBe(true);
    expect(second!.paused).toBe(false);
  });

  it('pauses every loop, drops its sources and shows the poster when reduced motion comes on', () => {
    const { videos, watcher, showInView, setReduced } = setup();
    const [first, second] = videos;
    showInView([first!]);
    setReduced(true);
    expect(watcher.disconnected).toBe(true);
    for (const video of videos) {
      expect(video.paused).toBe(true);
      expect(video.srcs()).toEqual([null, null]);
      expect(video.parentElement.classList.contains('is-playing')).toBe(false);
    }
    expect(first!.loads).toBe(2);
    expect(second!.loads).toBe(1);
    showInView([first!]);
    expect(first!.paused).toBe(true);
  });

  it('restores the sources when reduced motion goes, and plays the loops in view again', () => {
    const { videos, watcher, showInView, setReduced } = setup();
    const [first, second] = videos;
    showInView([first!]);
    setReduced(true);
    setReduced(false);
    expect(first!.srcs()).toEqual(['/media/any-file-light.webm', '/media/any-file-light.mp4']);
    expect(second!.srcs()).toEqual(['/media/search-light.webm', '/media/search-light.mp4']);
    expect(watcher.observed).toEqual(videos);
    showInView([first!]);
    expect(first!.paused).toBe(false);
    expect(first!.parentElement.classList.contains('is-playing')).toBe(true);
    expect(second!.paused).toBe(true);
  });

  it('observes nothing when the page loads under reduced motion, and starts when it goes', () => {
    const { videos, watcher, showInView, setReduced } = setup({ reduced: true });
    expect(watcher.observed).toEqual([]);
    expect(videos[0]!.srcs()).toEqual(['/media/any-file-light.webm', '/media/any-file-light.mp4']);
    setReduced(false);
    showInView([videos[1]!]);
    expect(videos[1]!.paused).toBe(false);
  });

  it('toggles a loop on click or Enter only while motion is allowed', () => {
    const { videos, setReduced } = setup();
    const video = videos[0]!;
    video.dispatch('click');
    expect(video.paused).toBe(false);
    video.dispatch('keydown', 'Enter');
    expect(video.paused).toBe(true);
    video.dispatch('keydown', 'a');
    expect(video.paused).toBe(true);
    setReduced(true);
    video.dispatch('click');
    video.dispatch('keydown', ' ');
    expect(video.paused).toBe(true);
  });
});
