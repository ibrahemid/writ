export const SCENES = ['hero', 'any-file', 'markdown', 'search', 'apps', 'versions'] as const;

export type SceneName = (typeof SCENES)[number];

export type SceneState = 'done' | 'cancelled';

export type FrameMessage =
  | { readonly type: 'writ-demo-ready' }
  | { readonly type: 'writ-demo-engaged' }
  | { readonly type: 'writ-demo-scene'; readonly name: SceneName; readonly state: SceneState };

export interface SceneGate {
  readonly live: boolean;
  readonly tour: boolean;
  readonly engaged: boolean;
  readonly visible: boolean;
}

export interface StepBox {
  readonly name: SceneName;
  readonly top: number;
  readonly atLine?: boolean;
}

export type PostScene = (scene: SceneName) => void;

export interface SceneDriver {
  readonly active: SceneName;
  setActive(scene: SceneName): void;
  setTour(on: boolean): void;
  setVisible(visible: boolean): void;
  ready(post: PostScene): void;
  engaged(): void;
}

export interface Tour {
  ready(post: PostScene): void;
  engaged(): void;
}

type StyleReader = Pick<CSSStyleDeclaration, 'getPropertyValue'>;

export interface TourEnv {
  readonly doc: Pick<Document, 'documentElement' | 'querySelector' | 'querySelectorAll' | 'visibilityState' | 'addEventListener'>;
  readonly matchMedia: (query: string) => Pick<MediaQueryList, 'matches' | 'addEventListener'>;
  readonly IntersectionObserver: new (
    callback: IntersectionObserverCallback,
    options?: IntersectionObserverInit,
  ) => Pick<IntersectionObserver, 'observe' | 'disconnect'>;
  readonly ResizeObserver: new (callback: ResizeObserverCallback) => Pick<ResizeObserver, 'observe'>;
  readonly getComputedStyle: (element: Element) => StyleReader;
  readonly viewport: Pick<Window, 'innerHeight' | 'addEventListener'>;
}

export class TourTokenError extends Error {
  constructor(readonly token: string, readonly raw: string) {
    super(`The tour reads ${token} as a length, and the page resolved it to "${raw}".`);
    this.name = 'TourTokenError';
  }
}

const SCENE_STATES: readonly SceneState[] = ['done', 'cancelled'];

export function isSceneName(value: unknown): value is SceneName {
  return typeof value === 'string' && (SCENES as readonly string[]).includes(value);
}

export function shouldPostScene(gate: SceneGate): boolean {
  return gate.live && gate.tour && !gate.engaged && gate.visible;
}

export function isFrameMessage(
  event: Pick<MessageEvent, 'source' | 'origin'>,
  frame: MessageEventSource | null,
  origin: string,
): boolean {
  return frame !== null && event.source === frame && event.origin === origin;
}

export function readFrameMessage(data: unknown): FrameMessage | null {
  if (typeof data !== 'object' || data === null) return null;
  const { type, name, state } = data as Record<string, unknown>;
  if (type === 'writ-demo-ready' || type === 'writ-demo-engaged') return { type };
  if (type !== 'writ-demo-scene' || !isSceneName(name)) return null;
  const known = SCENE_STATES.find((candidate) => candidate === state);
  return known ? { type, name, state: known } : null;
}

export function activeScene(line: number, steps: readonly StepBox[]): SceneName {
  let atLine: SceneName | null = null;
  for (const step of steps) if (step.atLine) atLine = step.name;
  if (atLine) return atLine;
  let active: SceneName = 'hero';
  for (const step of steps) {
    if (step.top > line) break;
    active = step.name;
  }
  return active;
}

export function createSceneDriver(initial: { readonly tour: boolean; readonly visible: boolean }): SceneDriver {
  let post: PostScene | null = null;
  let active: SceneName = 'hero';
  let posted: SceneName | null = null;
  let tour = initial.tour;
  let visible = initial.visible;
  let engaged = false;

  const send = (force: boolean): void => {
    if (!post || !shouldPostScene({ live: true, tour, engaged, visible })) return;
    if (!force && posted === active) return;
    posted = active;
    post(active);
  };

  return {
    get active() {
      return active;
    },
    setActive(scene) {
      if (scene === active) return;
      active = scene;
      send(false);
    },
    setTour(on) {
      if (on === tour) return;
      tour = on;
      send(false);
    },
    setVisible(next) {
      if (next === visible) return;
      visible = next;
      send(true);
    },
    ready(next) {
      post = next;
      send(true);
    },
    engaged() {
      engaged = true;
    },
  };
}

function readLength(style: StyleReader, token: string): number {
  const raw = style.getPropertyValue(token).trim();
  const value = Number.parseFloat(raw);
  if (!Number.isFinite(value)) throw new TourTokenError(token, raw);
  return value;
}

interface Step {
  readonly element: HTMLElement;
  readonly name: SceneName;
  readonly anchor: 'top' | 'bottom';
}

export function startTour(root: HTMLElement, bp: string, env: TourEnv): Tour {
  const { doc } = env;
  const html = doc.documentElement;
  const stage = doc.querySelector<HTMLElement>('.stage');
  const camera = root.querySelector<HTMLElement>('[data-live-camera]');
  const steps: Step[] = [];
  for (const element of doc.querySelectorAll<HTMLElement>('[data-step]')) {
    const name = element.dataset.step;
    if (isSceneName(name)) steps.push({ element, name, anchor: element.dataset.anchor === 'bottom' ? 'bottom' : 'top' });
  }

  const wide = env.matchMedia(`(min-width: ${bp})`);
  const reduced = env.matchMedia('(prefers-reduced-motion: reduce)');
  const isTourOn = (): boolean => wide.matches && !reduced.matches;
  const driver = createSceneDriver({ tour: isTourOn(), visible: doc.visibilityState === 'visible' });

  const line = (): number => {
    const style = env.getComputedStyle(html);
    return readLength(style, '--writ-site-nav-height') + readLength(style, '--writ-site-tour-band');
  };

  const atLine = new Set<Element>();
  const update = (): void => {
    const scene = activeScene(
      line(),
      steps.map((step) => ({
        name: step.name,
        top: step.element.getBoundingClientRect().top,
        atLine: atLine.has(step.element),
      })),
    );
    let anchor: Step['anchor'] = 'top';
    for (const step of steps) {
      step.element.classList.toggle('is-active', step.name === scene);
      if (step.name === scene) anchor = step.anchor;
    }
    if (stage) stage.dataset.anchor = anchor;
    driver.setActive(scene);
  };

  let observer: Pick<IntersectionObserver, 'observe' | 'disconnect'> | null = null;
  const observe = (): void => {
    observer?.disconnect();
    atLine.clear();
    const top = Math.round(line());
    const bottom = Math.max(0, Math.round(env.viewport.innerHeight) - top - 1);
    const onEntries: IntersectionObserverCallback = (entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) atLine.add(entry.target);
        else atLine.delete(entry.target);
      }
      update();
    };
    observer = new env.IntersectionObserver(onEntries, { rootMargin: `-${top}px 0% -${bottom}px 0%` });
    for (const step of steps) observer.observe(step.element);
  };

  const syncTour = (): void => {
    const on = isTourOn();
    html.toggleAttribute('data-writ-tour', on);
    if (!on) driver.setTour(false);
    observe();
    update();
    if (on) driver.setTour(true);
  };

  if (camera) {
    const crop = (): void => {
      root.classList.toggle('is-cropped', camera.offsetHeight > root.clientHeight + 1);
    };
    const sizes = new env.ResizeObserver(crop);
    sizes.observe(root);
    sizes.observe(camera);
  }

  syncTour();
  wide.addEventListener('change', syncTour);
  reduced.addEventListener('change', syncTour);
  env.viewport.addEventListener('resize', () => {
    observe();
    update();
  });
  doc.addEventListener('visibilitychange', () => driver.setVisible(doc.visibilityState === 'visible'));

  return {
    ready: (post) => driver.ready(post),
    engaged: () => driver.engaged(),
  };
}
