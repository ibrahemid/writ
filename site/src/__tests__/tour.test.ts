import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  SCENES,
  findActiveScene,
  createSceneDriver,
  isFrameMessage,
  readFrameMessage,
  shouldPostScene,
  startTour,
  TourTokenError,
  type SceneName,
  type TourEnv,
} from '../scripts/tour';

const NAV = 56;
const BAND = 144;
const LINE = NAV + BAND;

describe('SCENES', () => {
  const SITE = process.cwd();

  it('names the scenes the demo runs, in its order', () => {
    const types = readFileSync(join(SITE, '..', 'demo', 'scenes', 'types.ts'), 'utf8');
    const list = /export const SCENE_NAMES = \[([^\]]*)\] as const;/.exec(types)?.[1];
    expect(list, 'demo/scenes/types.ts declares no SCENE_NAMES').toBeDefined();
    const names = [...(list ?? '').matchAll(/["']([a-z-]+)["']/g)].map((match) => match[1]);
    expect(SCENES).toEqual(names);
  });

  it('is hero followed by the Feature ids on the landing page, in page order', () => {
    const index = readFileSync(join(SITE, 'src', 'pages', 'index.astro'), 'utf8');
    const ids = [...index.matchAll(/<Feature id="([a-z-]+)"/g)].map((match) => match[1]);
    expect(SCENES).toEqual(['hero', ...ids]);
  });
});

describe('shouldPostScene', () => {
  it('passes only when the frame is live, the tour is on, the visitor has not engaged and the page is visible', () => {
    for (const isLive of [true, false]) {
      for (const isTourOn of [true, false]) {
        for (const hasEngaged of [true, false]) {
          for (const isVisible of [true, false]) {
            const gate = { isLive, isTourOn, hasEngaged, isVisible };
            expect(shouldPostScene(gate), JSON.stringify(gate)).toBe(isLive && isTourOn && !hasEngaged && isVisible);
          }
        }
      }
    }
  });
});

describe('isFrameMessage', () => {
  const frame = {} as MessageEventSource;
  const other = {} as MessageEventSource;
  const origin = 'https://writ.ibrahemid.com';

  it('accepts a message from the frame on the page origin', () => {
    expect(isFrameMessage({ source: frame, origin }, frame, origin)).toBe(true);
  });

  it('rejects another window, another origin, and a frame that has no window yet', () => {
    expect(isFrameMessage({ source: other, origin }, frame, origin)).toBe(false);
    expect(isFrameMessage({ source: frame, origin: 'https://example.com' }, frame, origin)).toBe(false);
    expect(isFrameMessage({ source: null, origin }, null, origin)).toBe(false);
  });
});

describe('readFrameMessage', () => {
  it('reads the three messages the app sends', () => {
    expect(readFrameMessage({ type: 'writ-demo-ready' })).toEqual({ type: 'writ-demo-ready' });
    expect(readFrameMessage({ type: 'writ-demo-engaged' })).toEqual({ type: 'writ-demo-engaged' });
    expect(readFrameMessage({ type: 'writ-demo-scene', name: 'search', state: 'done' })).toEqual({
      type: 'writ-demo-scene',
      name: 'search',
      state: 'done',
    });
  });

  it('drops anything else', () => {
    for (const data of [null, 'writ-demo-ready', 3, {}, { type: 'writ-demo-scene', name: 'graph', state: 'done' }, { type: 'writ-demo-scene', name: 'apps', state: 'late' }, { type: 'other' }]) {
      expect(readFrameMessage(data), JSON.stringify(data)).toBeNull();
    }
  });
});

describe('findActiveScene', () => {
  const steps = (first: number) =>
    SCENES.slice(1).map((name, i) => ({ name, top: first + i * 800 }));

  it('is hero above the first step', () => {
    expect(findActiveScene(LINE, steps(LINE + 1))).toBe('hero');
    expect(findActiveScene(LINE, [])).toBe('hero');
  });

  it('is the last step whose top has reached the line', () => {
    expect(findActiveScene(LINE, steps(LINE))).toBe('any-file');
    expect(findActiveScene(LINE, steps(LINE - 1700))).toBe('search');
  });

  it('keeps versions once the line is past the stage', () => {
    expect(findActiveScene(LINE, steps(-100000))).toBe('versions');
  });

  it('trusts the step the observer reports at the line over a rect a fraction of a pixel off it', () => {
    const boxes = steps(LINE + 0.77).map((box) => ({ ...box, isAtLine: box.name === 'any-file' }));
    expect(findActiveScene(LINE, boxes)).toBe('any-file');
  });

  it('takes the later of two steps that both touch the line', () => {
    const boxes = steps(LINE - 800).map((box) => ({ ...box, isAtLine: box.name === 'any-file' || box.name === 'markdown' }));
    expect(findActiveScene(LINE, boxes)).toBe('markdown');
  });
});

describe('createSceneDriver', () => {
  const posts = () => {
    const sent: SceneName[] = [];
    return { sent, post: (scene: SceneName) => sent.push(scene) };
  };

  it('posts nothing before the app is ready, then the active scene when it is', () => {
    const driver = createSceneDriver({ isTourOn: true, isVisible: true });
    const { sent, post } = posts();
    driver.setActiveScene('markdown');
    expect(sent).toEqual([]);
    driver.connectFrame(post);
    expect(sent).toEqual(['markdown']);
  });

  it('posts each change once and never repeats a scene', () => {
    const driver = createSceneDriver({ isTourOn: true, isVisible: true });
    const { sent, post } = posts();
    driver.connectFrame(post);
    driver.setActiveScene('any-file');
    driver.setActiveScene('any-file');
    driver.setActiveScene('markdown');
    driver.setActiveScene('markdown');
    expect(sent).toEqual(['hero', 'any-file', 'markdown']);
  });

  it('goes silent for good once the visitor engages', () => {
    const driver = createSceneDriver({ isTourOn: true, isVisible: true });
    const { sent, post } = posts();
    driver.connectFrame(post);
    driver.markEngaged();
    driver.setActiveScene('search');
    driver.setVisible(false);
    driver.setVisible(true);
    driver.connectFrame(post);
    expect(sent).toEqual(['hero']);
  });

  it('holds changes while the page is hidden and posts the active scene when it shows again', () => {
    const driver = createSceneDriver({ isTourOn: true, isVisible: true });
    const { sent, post } = posts();
    driver.connectFrame(post);
    driver.setVisible(false);
    driver.setActiveScene('any-file');
    driver.setActiveScene('apps');
    expect(sent).toEqual(['hero']);
    driver.setVisible(true);
    expect(sent).toEqual(['hero', 'apps']);
  });

  it('rests the frame on hero once it is live and not engaged, hidden or not, then resumes the active scene', () => {
    const driver = createSceneDriver({ isTourOn: true, isVisible: true });
    const { sent, post } = posts();
    driver.restFrame();
    expect(sent).toEqual([]);
    driver.connectFrame(post);
    driver.setActiveScene('apps');
    driver.setVisible(false);
    driver.restFrame();
    driver.setTourOn(false);
    expect(sent).toEqual(['hero', 'apps', 'hero']);
    driver.setVisible(true);
    driver.setTourOn(true);
    expect(sent).toEqual(['hero', 'apps', 'hero', 'apps']);
    driver.markEngaged();
    driver.restFrame();
    expect(sent).toEqual(['hero', 'apps', 'hero', 'apps']);
  });

  it('posts nothing while the tour is off, and the active scene when it comes back', () => {
    const driver = createSceneDriver({ isTourOn: false, isVisible: true });
    const { sent, post } = posts();
    driver.connectFrame(post);
    driver.setActiveScene('versions');
    expect(sent).toEqual([]);
    driver.setTourOn(true);
    expect(sent).toEqual(['versions']);
    driver.setTourOn(false);
    driver.setTourOn(true);
    expect(sent).toEqual(['versions']);
  });
});

class FakeClassList {
  readonly names = new Set<string>();
  toggle(name: string, force?: boolean): boolean {
    const on = force ?? !this.names.has(name);
    if (on) this.names.add(name);
    else this.names.delete(name);
    return on;
  }
  contains(name: string): boolean {
    return this.names.has(name);
  }
}

interface FakeElement {
  dataset: Record<string, string | undefined>;
  classList: FakeClassList;
  top: number;
  offsetHeight: number;
  clientHeight: number;
  getBoundingClientRect(): { top: number };
  querySelector(selector: string): FakeElement | null;
}

function element(dataset: Record<string, string> = {}, children: Record<string, FakeElement> = {}): FakeElement {
  return {
    dataset: { ...dataset },
    classList: new FakeClassList(),
    top: 0,
    offsetHeight: 0,
    clientHeight: 0,
    getBoundingClientRect() {
      return { top: this.top };
    },
    querySelector: (selector) => children[selector] ?? null,
  };
}

function setup(options: { wide?: boolean; reduced?: boolean; tokens?: Record<string, string> } = {}) {
  const attributes = new Set<string>();
  const layout = { onTour: (_on: boolean): void => {} };
  const html = {
    toggleAttribute(name: string, force?: boolean) {
      const on = force ?? !attributes.has(name);
      if (on) attributes.add(name);
      else attributes.delete(name);
      if (name === 'data-writ-tour') layout.onTour(on);
      return on;
    },
  };
  const stage = element();
  const anchors: Partial<Record<SceneName, string>> = { markdown: 'bottom', versions: 'bottom' };
  const steps = SCENES.slice(1).map((name) => element({ step: name, anchor: anchors[name] ?? 'top' }));
  steps.forEach((step, i) => {
    step.top = 900 + i * 800;
  });
  const camera = element();
  const root = element({}, { '[data-live-camera]': camera });
  const listeners: Record<string, (() => void)[]> = {};
  const media: Record<string, { matches: boolean; change: (() => void)[] }> = {};
  const observers: {
    callback: (entries: { target: unknown; isIntersecting: boolean }[]) => void;
    options?: IntersectionObserverInit;
    observed: unknown[];
    disconnected: boolean;
  }[] = [];
  const resized: (() => void)[] = [];
  const timers = new Map<number, () => void>();
  let nextTimer = 1;
  const doc = {
    documentElement: html,
    visibilityState: 'visible' as DocumentVisibilityState,
    querySelector: (selector: string) => (selector === '.stage' ? stage : null),
    querySelectorAll: (selector: string) => (selector === '[data-step]' ? steps : []),
    addEventListener: (type: string, fn: () => void) => {
      (listeners[type] ??= []).push(fn);
    },
  };
  const tokens = options.tokens ?? { '--writ-site-nav-height': `${NAV}px`, '--writ-site-tour-band': `${BAND}px` };
  const env = {
    doc,
    matchMedia: (query: string) => {
      const entry = (media[query] ??= {
        matches: query.includes('reduce') ? Boolean(options.reduced) : options.wide ?? true,
        change: [],
      });
      return { get matches() { return entry.matches; }, addEventListener: (_: string, fn: () => void) => entry.change.push(fn) };
    },
    IntersectionObserver: class {
      readonly record;
      constructor(callback: (entries: { target: unknown; isIntersecting: boolean }[]) => void, init?: IntersectionObserverInit) {
        this.record = { callback, options: init, observed: [] as unknown[], disconnected: false };
        observers.push(this.record);
      }
      observe(target: unknown) {
        this.record.observed.push(target);
      }
      disconnect() {
        this.record.disconnected = true;
      }
    },
    ResizeObserver: class {
      constructor(callback: () => void) {
        resized.push(callback);
      }
      observe() {}
    },
    getComputedStyle: () => ({ getPropertyValue: (name: string) => tokens[name] ?? '' }),
    viewport: {
      innerHeight: 800,
      addEventListener: (type: string, fn: () => void) => {
        (listeners[`window:${type}`] ??= []).push(fn);
      },
      setTimeout: (fn: () => void) => {
        const id = nextTimer++;
        timers.set(id, fn);
        return id;
      },
      clearTimeout: (id: number) => {
        timers.delete(id);
      },
    },
  };
  // An IntersectionObserver reports every observed step once after it starts,
  // then again whenever a step crosses its root margin.
  const deliver = (): void => {
    observers.at(-1)?.callback(
      steps.map((step) => ({ target: step, isIntersecting: step.top <= LINE + 1 && step.top + 800 > LINE })),
    );
  };
  const scroll = (y: number, offset = 0): void => {
    steps.forEach((step, i) => {
      step.top = 900 + i * 800 - y + offset;
    });
    deliver();
  };
  const runTimers = (): void => {
    const due = [...timers.values()];
    timers.clear();
    for (const fn of due) fn();
  };
  const fire = (type: string): void => {
    for (const fn of listeners[type] ?? []) fn();
  };
  const flip = (query: string, matches: boolean): void => {
    const entry = media[query];
    if (!entry) throw new Error(`no media query ${query}`);
    entry.matches = matches;
    for (const fn of entry.change) fn();
  };
  const tour = startTour(root as unknown as HTMLElement, '1200px', env as unknown as TourEnv);
  return { tour, attributes, layout, stage, steps, camera, root, observers, resized, doc, env, scroll, deliver, runTimers, fire, flip };
}

describe('startTour', () => {
  it('draws the trigger line at the bottom of the caption band, and redraws it on resize', () => {
    const { observers, env, fire, runTimers, steps } = setup();
    expect(observers[0]?.options?.rootMargin).toBe(`-${LINE}px 0% -${800 - LINE - 1}px 0%`);
    expect(observers[0]?.observed).toEqual(steps);
    env.viewport.innerHeight = 900;
    fire('window:resize');
    runTimers();
    expect(observers[0]?.disconnected).toBe(true);
    expect(observers[1]?.options?.rootMargin).toBe(`-${LINE}px 0% -${900 - LINE - 1}px 0%`);
  });

  it('posts nothing new when a resize rebuilds the observer while a step sits a fraction of a pixel off the line', () => {
    const { tour, scroll, deliver, runTimers, fire, env, observers } = setup();
    const sent: SceneName[] = [];
    tour.connectFrame((scene) => sent.push(scene));
    scroll(900 - LINE + 800, 0.4);
    expect(sent).toEqual(['hero', 'markdown']);
    for (const height of [801, 800, 799, 800]) {
      env.viewport.innerHeight = height;
      fire('window:resize');
    }
    expect(observers).toHaveLength(1);
    expect(sent).toEqual(['hero', 'markdown']);
    runTimers();
    expect(observers).toHaveLength(2);
    expect(sent).toEqual(['hero', 'markdown']);
    deliver();
    expect(sent).toEqual(['hero', 'markdown']);
  });

  it('ignores entries an observer queued before a resize replaced it', () => {
    const { tour, scroll, runTimers, fire, observers, steps } = setup();
    const sent: SceneName[] = [];
    tour.connectFrame((scene) => sent.push(scene));
    scroll(900 - LINE + 800);
    fire('window:resize');
    runTimers();
    observers[0]?.callback(steps.map((step, i) => ({ target: step, isIntersecting: i === 3 })));
    expect(sent).toEqual(['hero', 'markdown']);
  });

  it('anchors the camera to the part of the frame the step at the line needs', () => {
    const { scroll, stage } = setup();
    scroll(0);
    expect(stage.dataset.anchor).toBe('top');
    scroll(900 - LINE);
    expect(stage.dataset.anchor).toBe('top');
    scroll(900 - LINE + 1 * 800);
    expect(stage.dataset.anchor).toBe('bottom');
    scroll(900 - LINE + 2 * 800);
    expect(stage.dataset.anchor).toBe('top');
    scroll(900 - LINE + 3 * 800);
    expect(stage.dataset.anchor).toBe('top');
    scroll(900 - LINE + 4 * 800);
    expect(stage.dataset.anchor).toBe('bottom');
  });

  it('posts scenes through the ready callback as the steps cross the line', () => {
    const { tour, scroll } = setup();
    const sent: SceneName[] = [];
    scroll(0);
    tour.connectFrame((scene) => sent.push(scene));
    scroll(900 - LINE);
    scroll(900 - LINE + 10);
    scroll(900 - LINE + 4 * 800 + 5000);
    expect(sent).toEqual(['hero', 'any-file', 'versions']);
  });

  it('moves to a step whose top the observer puts on the line even when its rect sits just below it', () => {
    const { tour, scroll } = setup();
    const sent: SceneName[] = [];
    tour.connectFrame((scene) => sent.push(scene));
    scroll(900 - LINE, 0.77);
    expect(sent).toEqual(['hero', 'any-file']);
  });

  it('posts the active step when ready arrives late', () => {
    const { tour, scroll } = setup();
    const sent: SceneName[] = [];
    scroll(900 - LINE + 2 * 800);
    tour.connectFrame((scene) => sent.push(scene));
    expect(sent).toEqual(['search']);
  });

  it('stops posting once engaged, and holds while the page is hidden', () => {
    const hidden = setup();
    const held: SceneName[] = [];
    hidden.tour.connectFrame((scene) => held.push(scene));
    hidden.doc.visibilityState = 'hidden';
    hidden.fire('visibilitychange');
    hidden.scroll(900 - LINE + 800);
    expect(held).toEqual(['hero']);
    hidden.doc.visibilityState = 'visible';
    hidden.fire('visibilitychange');
    expect(held).toEqual(['hero', 'markdown']);

    const engaged = setup();
    const quiet: SceneName[] = [];
    engaged.tour.connectFrame((scene) => quiet.push(scene));
    engaged.tour.markEngaged();
    engaged.scroll(900 - LINE + 800);
    expect(quiet).toEqual(['hero']);
  });

  it('turns the tour attribute off below the breakpoint or under reduced motion, and back on', () => {
    const { attributes, flip, tour, scroll, deliver } = setup();
    const sent: SceneName[] = [];
    tour.connectFrame((scene) => sent.push(scene));
    expect(attributes.has('data-writ-tour')).toBe(true);
    flip('(min-width: 1200px)', false);
    expect(attributes.has('data-writ-tour')).toBe(false);
    scroll(900 - LINE);
    expect(sent).toEqual(['hero']);
    flip('(min-width: 1200px)', true);
    expect(attributes.has('data-writ-tour')).toBe(true);
    deliver();
    expect(sent).toEqual(['hero', 'any-file']);
    flip('(prefers-reduced-motion: reduce)', true);
    expect(attributes.has('data-writ-tour')).toBe(false);
  });

  it('posts nothing as the tour turns off, and the step at the line in the tour layout when it comes back', () => {
    const { tour, flip, layout, steps, observers, deliver } = setup();
    const sent: SceneName[] = [];
    tour.connectFrame((scene) => sent.push(scene));
    layout.onTour = (on) => {
      steps.forEach((step, i) => {
        step.top = on ? LINE - 800 + i * 800 : LINE - (steps.length - 1 - i) * 400;
      });
    };
    flip('(min-width: 1200px)', false);
    expect(sent).toEqual(['hero']);
    observers.at(-1)?.callback(steps.map((step) => ({ target: step, isIntersecting: step.top <= LINE + 1 && step.top + 400 > LINE })));
    expect(sent).toEqual(['hero']);
    flip('(min-width: 1200px)', true);
    expect(sent).toEqual(['hero']);
    deliver();
    expect(sent).toEqual(['hero', 'markdown']);
  });

  it('rests the frame on hero before the tour turns off when reduced motion comes on mid-scene', () => {
    const { tour, flip, layout, scroll, attributes, deliver } = setup();
    const log: string[] = [];
    tour.connectFrame((scene) => log.push(scene));
    layout.onTour = (on) => log.push(on ? 'tour on' : 'tour off');
    scroll(900 - LINE + 800);
    flip('(prefers-reduced-motion: reduce)', true);
    expect(log).toEqual(['hero', 'markdown', 'hero', 'tour off']);
    expect(attributes.has('data-writ-tour')).toBe(false);
    scroll(900 - LINE + 2 * 800);
    expect(log).toEqual(['hero', 'markdown', 'hero', 'tour off']);
    flip('(prefers-reduced-motion: reduce)', false);
    deliver();
    expect(log).toEqual(['hero', 'markdown', 'hero', 'tour off', 'tour on', 'search']);
  });

  it('leaves an engaged frame alone when reduced motion comes on', () => {
    const { tour, flip, scroll } = setup();
    const sent: SceneName[] = [];
    tour.connectFrame((scene) => sent.push(scene));
    scroll(900 - LINE + 800);
    tour.markEngaged();
    flip('(prefers-reduced-motion: reduce)', true);
    expect(sent).toEqual(['hero', 'markdown']);
  });

  it('starts with the tour off under reduced motion and posts nothing', () => {
    const { attributes, tour } = setup({ reduced: true });
    const sent: SceneName[] = [];
    tour.connectFrame((scene) => sent.push(scene));
    expect(attributes.has('data-writ-tour')).toBe(false);
    expect(sent).toEqual([]);
  });

  it('marks the window cropped only while the camera is taller than it', () => {
    const { root, camera, resized } = setup();
    camera.offsetHeight = 750;
    root.clientHeight = 750;
    resized[0]?.();
    expect(root.classList.contains('is-cropped')).toBe(false);
    root.clientHeight = 542;
    resized[0]?.();
    expect(root.classList.contains('is-cropped')).toBe(true);
  });

  it('fails loudly when a band token is missing', () => {
    expect(() => setup({ tokens: { '--writ-site-nav-height': `${NAV}px` } })).toThrow(TourTokenError);
  });
});
