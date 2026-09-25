export const SCENES = ['hero', 'any-file', 'markdown', 'search', 'apps', 'versions'] as const;

export type SceneName = (typeof SCENES)[number];

export type SceneState = 'done' | 'cancelled';

export type CameraAnchor = 'top' | 'bottom';

const CAMERA_ANCHORS: readonly CameraAnchor[] = ['top', 'bottom'];

const RESIZE_SETTLE_MS = 150;

export type FrameMessage =
  | { readonly type: 'writ-demo-ready' }
  | { readonly type: 'writ-demo-engaged' }
  | { readonly type: 'writ-demo-scene'; readonly name: SceneName; readonly state: SceneState };

export interface SceneGate {
  readonly isLive: boolean;
  readonly isTourOn: boolean;
  readonly hasEngaged: boolean;
  readonly isVisible: boolean;
}

export interface StepBox {
  readonly name: SceneName;
  readonly top: number;
  readonly isAtLine?: boolean;
}

export type PostScene = (scene: SceneName) => void;

export interface SceneDriver {
  readonly active: SceneName;
  setActiveScene(scene: SceneName): void;
  setTourOn(isOn: boolean): void;
  setVisible(isVisible: boolean): void;
  restFrame(): void;
  connectFrame(post: PostScene): void;
  markEngaged(): void;
}

export interface Tour {
  connectFrame(post: PostScene): void;
  markEngaged(): void;
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
  readonly viewport: Pick<Window, 'innerHeight' | 'addEventListener' | 'setTimeout' | 'clearTimeout'>;
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
  return gate.isLive && gate.isTourOn && !gate.hasEngaged && gate.isVisible;
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

export function findActiveScene(line: number, steps: readonly StepBox[]): SceneName {
  let sceneAtLine: SceneName | null = null;
  for (const step of steps) if (step.isAtLine) sceneAtLine = step.name;
  if (sceneAtLine) return sceneAtLine;
  let active: SceneName = 'hero';
  for (const step of steps) {
    if (step.top > line) break;
    active = step.name;
  }
  return active;
}

export function createSceneDriver(initial: { readonly isTourOn: boolean; readonly isVisible: boolean }): SceneDriver {
  let post: PostScene | null = null;
  let active: SceneName = 'hero';
  let posted: SceneName | null = null;
  let isTourOn = initial.isTourOn;
  let isVisible = initial.isVisible;
  let hasEngaged = false;

  const sendScene = (isForced: boolean): void => {
    if (!post || !shouldPostScene({ isLive: true, isTourOn, hasEngaged, isVisible })) return;
    if (!isForced && posted === active) return;
    posted = active;
    post(active);
  };

  return {
    get active() {
      return active;
    },
    setActiveScene(scene) {
      if (scene === active) return;
      active = scene;
      sendScene(false);
    },
    setTourOn(isOn) {
      if (isOn === isTourOn) return;
      isTourOn = isOn;
      sendScene(false);
    },
    setVisible(isNowVisible) {
      if (isNowVisible === isVisible) return;
      isVisible = isNowVisible;
      sendScene(true);
    },
    restFrame() {
      if (!post || hasEngaged) return;
      posted = 'hero';
      post('hero');
    },
    connectFrame(next) {
      post = next;
      sendScene(true);
    },
    markEngaged() {
      hasEngaged = true;
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
  readonly anchor: CameraAnchor;
}

function readAnchor(value: string | undefined): CameraAnchor {
  return CAMERA_ANCHORS.find((anchor) => anchor === value) ?? 'top';
}

export function startTour(root: HTMLElement, bp: string, env: TourEnv): Tour {
  const { doc } = env;
  const html = doc.documentElement;
  const stage = doc.querySelector<HTMLElement>('.stage');
  const camera = root.querySelector<HTMLElement>('[data-live-camera]');
  const steps: Step[] = [];
  for (const element of doc.querySelectorAll<HTMLElement>('[data-step]')) {
    const name = element.dataset.step;
    if (isSceneName(name)) steps.push({ element, name, anchor: readAnchor(element.dataset.anchor) });
  }

  const wideQuery = env.matchMedia(`(min-width: ${bp})`);
  const reducedQuery = env.matchMedia('(prefers-reduced-motion: reduce)');
  const isTourOn = (): boolean => wideQuery.matches && !reducedQuery.matches;
  const driver = createSceneDriver({ isTourOn: isTourOn(), isVisible: doc.visibilityState === 'visible' });

  const readTriggerLine = (): number => {
    const style = env.getComputedStyle(html);
    return readLength(style, '--writ-site-nav-height') + readLength(style, '--writ-site-tour-band');
  };

  const stepsAtLine = new Set<Element>();
  const updateActiveScene = (): void => {
    const scene = findActiveScene(
      readTriggerLine(),
      steps.map((step) => ({
        name: step.name,
        top: step.element.getBoundingClientRect().top,
        isAtLine: stepsAtLine.has(step.element),
      })),
    );
    let anchor: CameraAnchor = 'top';
    for (const step of steps) if (step.name === scene) anchor = step.anchor;
    if (stage) stage.dataset.anchor = anchor;
    driver.setActiveScene(scene);
  };

  // Until a rebuilt observer reports, the previous scene holds: a rect read in
  // between can sit a fraction of a pixel off the line and post a stray scene.
  let observer: Pick<IntersectionObserver, 'observe' | 'disconnect'> | null = null;
  let generation = 0;
  let isTourPending = false;
  const stopObserving = (): void => {
    observer?.disconnect();
    observer = null;
    generation += 1;
    stepsAtLine.clear();
  };
  const observeSteps = (): void => {
    stopObserving();
    const current = generation;
    const top = Math.round(readTriggerLine());
    const bottom = Math.max(0, Math.round(env.viewport.innerHeight) - top - 1);
    const onEntries: IntersectionObserverCallback = (entries) => {
      if (current !== generation) return;
      for (const entry of entries) {
        if (entry.isIntersecting) stepsAtLine.add(entry.target);
        else stepsAtLine.delete(entry.target);
      }
      updateActiveScene();
      if (isTourPending) {
        isTourPending = false;
        driver.setTourOn(true);
      }
    };
    observer = new env.IntersectionObserver(onEntries, { rootMargin: `-${top}px 0% -${bottom}px 0%` });
    for (const step of steps) observer.observe(step.element);
  };

  const syncTour = (): void => {
    const isOn = isTourOn();
    html.toggleAttribute('data-writ-tour', isOn);
    isTourPending = isOn;
    if (!isOn) driver.setTourOn(false);
    observeSteps();
  };

  if (camera) {
    const updateCrop = (): void => {
      root.classList.toggle('is-cropped', camera.offsetHeight > root.clientHeight + 1);
    };
    const sizeObserver = new env.ResizeObserver(updateCrop);
    sizeObserver.observe(root);
    sizeObserver.observe(camera);
  }

  syncTour();
  wideQuery.addEventListener('change', syncTour);
  reducedQuery.addEventListener('change', () => {
    if (reducedQuery.matches) driver.restFrame();
    syncTour();
  });
  let resizeTimer: number | null = null;
  env.viewport.addEventListener('resize', () => {
    stopObserving();
    if (resizeTimer !== null) env.viewport.clearTimeout(resizeTimer);
    resizeTimer = env.viewport.setTimeout(() => {
      resizeTimer = null;
      observeSteps();
    }, RESIZE_SETTLE_MS);
  });
  doc.addEventListener('visibilitychange', () => driver.setVisible(doc.visibilityState === 'visible'));

  return {
    connectFrame: (post) => driver.connectFrame(post),
    markEngaged: () => driver.markEngaged(),
  };
}
