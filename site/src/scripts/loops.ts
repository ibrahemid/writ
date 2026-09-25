export interface LoopEnv {
  readonly videos: readonly HTMLVideoElement[];
  readonly reducedQuery: Pick<MediaQueryList, 'matches' | 'addEventListener'>;
  readonly IntersectionObserver: new (callback: IntersectionObserverCallback) => Pick<IntersectionObserver, 'observe' | 'disconnect'>;
  readonly getComputedStyle: (element: Element) => Pick<CSSStyleDeclaration, 'opacity'>;
}

export function startLoopPlayback(env: LoopEnv): void {
  const { videos, reducedQuery } = env;
  const primed = new WeakSet<HTMLVideoElement>();

  const primeLoop = (video: HTMLVideoElement): void => {
    primed.add(video);
    const figure = video.parentElement;
    if (figure && video.poster) {
      figure.style.setProperty('--writ-loop-poster', `url("${video.poster}")`);
      figure.dataset.fade = '';
      // Commits the hidden state now: if `playing` lands before the next style pass, is-playing would
      // otherwise go from 1 to 1 and frame 1 would cut in over the poster instead of fading.
      void env.getComputedStyle(video).opacity;
      video.addEventListener('playing', () => figure.classList.add('is-playing'), { once: true });
    }
    video.load();
  };

  const watcher = new env.IntersectionObserver((entries) => {
    for (const entry of entries) {
      const video = entry.target as HTMLVideoElement;
      if (!entry.isIntersecting) {
        video.pause();
        continue;
      }
      if (!primed.has(video)) primeLoop(video);
      void video.play().catch(() => {});
    }
  });

  const resumeLoops = (): void => {
    for (const video of videos) {
      for (const source of video.querySelectorAll('source')) {
        const src = source.dataset.src;
        if (!src) continue;
        source.setAttribute('src', src);
        delete source.dataset.src;
      }
      watcher.observe(video);
    }
  };

  // A paused video keeps downloading. Dropping each source's src and reloading stops it and shows the poster.
  const stopLoops = (): void => {
    watcher.disconnect();
    for (const video of videos) {
      video.pause();
      for (const source of video.querySelectorAll('source')) {
        const src = source.getAttribute('src');
        if (!src) continue;
        source.dataset.src = src;
        source.removeAttribute('src');
      }
      video.load();
      primed.delete(video);
      video.parentElement?.classList.remove('is-playing');
    }
  };

  const togglePlayback = (video: HTMLVideoElement): void => {
    if (reducedQuery.matches) return;
    if (video.paused) void video.play().catch(() => {});
    else video.pause();
  };

  for (const video of videos) {
    video.addEventListener('click', () => togglePlayback(video));
    video.addEventListener('keydown', (event) => {
      if (event.key !== 'Enter' && event.key !== ' ') return;
      event.preventDefault();
      togglePlayback(video);
    });
  }

  reducedQuery.addEventListener('change', () => {
    if (reducedQuery.matches) stopLoops();
    else resumeLoops();
  });
  if (!reducedQuery.matches) resumeLoops();
}
