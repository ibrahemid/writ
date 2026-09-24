import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const SITE = process.cwd();
const COMPONENT = readFileSync(join(SITE, 'src', 'components', 'LiveWindow.astro'), 'utf8');
const CSS = readFileSync(join(SITE, 'src', 'styles', 'site.css'), 'utf8');

describe('the live window', () => {
  it('renders the hero still eagerly, so it is the page without JS and the LCP image', () => {
    expect(COMPONENT).toMatch(/<Capture name="hero-window" alt=\{alt\} loading="eager" fetchpriority="high" \/>/);
  });

  it('loads the app only once the page is idle and the column is at the large breakpoint', () => {
    expect(COMPONENT).toContain("getPropertyValue('--writ-site-bp-lg')");
    expect(COMPONENT).toContain('requestIdleCallback');
    expect(COMPONENT).toContain("frame.src = '/demo/'");
  });

  it('shows the app only when it says it is ready, from its own window and origin', () => {
    expect(COMPONENT).toContain('event.source !== frame.contentWindow || event.origin !== location.origin');
    expect(COMPONENT).toContain("'writ-demo-ready'");
    expect(CSS).toContain('.live-window.is-live .live-frame {\n  opacity: 1;');
  });

  it('takes the still out of sight and out of the accessibility tree once the app is up', () => {
    expect(CSS).toMatch(/\.live-window\.is-live \.window \{\n  visibility: hidden;/);
    expect(COMPONENT).toContain("root.querySelector('.window')?.setAttribute('aria-hidden', 'true')");
  });

  it('scales a frame the size of the hero capture to the column', () => {
    expect(COMPONENT).toContain('const FRAME_WIDTH = 1440;');
    expect(COMPONENT).toContain('const FRAME_HEIGHT = 900;');
    expect(COMPONENT).toContain('scale(${root.clientWidth / width})');
  });
});
