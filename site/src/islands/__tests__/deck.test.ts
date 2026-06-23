import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { createElement } from 'react';

// Mock motion/react before importing Deck
vi.mock('motion/react', () => ({
  useReducedMotion: vi.fn(() => true),
  useScroll: vi.fn(() => ({ scrollYProgress: { get: () => 0 } })),
  useSpring: vi.fn((v: unknown) => v),
  useTransform: vi.fn(() => ({ get: () => 0 })),
  useMotionValueEvent: vi.fn(),
  motion: {
    div: 'div',
    span: 'span',
  },
}));

// Import after mocks are registered
import Deck from '../Deck.js';

let container: HTMLElement;
let root: ReturnType<typeof createRoot>;

beforeEach(() => {
  container = document.createElement('div');
  document.body.appendChild(container);

  // Full MediaQueryList mock — answers both reduced-motion and 880px queries with true
  Object.defineProperty(window, 'matchMedia', {
    writable: true,
    value: (query: string) => ({
      matches: query.includes('prefers-reduced-motion') || query.includes('880px'),
      media: query,
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }),
  });
});

afterEach(() => {
  act(() => { root?.unmount(); });
  document.body.removeChild(container);
});

describe('Deck (reduced-motion fallback)', () => {
  it('renders all 5 scenes as .rcard elements when reduced-motion is on', async () => {
    await act(async () => {
      root = createRoot(container);
      root.render(createElement(Deck, null));
    });

    const rcards = container.querySelectorAll('.rcard');
    expect(rcards.length).toBe(5);
  });

  it('renders stacked fallback structure and omits motion-only markup when reduced-motion is on', async () => {
    await act(async () => {
      root = createRoot(container);
      root.render(createElement(Deck, null));
    });

    // Stacked path: section must carry the "stacked" class
    const section = container.querySelector('.deck.stacked');
    expect(section).not.toBeNull();

    // Stacked path: all 5 .rcard elements must be present
    const rcards = container.querySelectorAll('.rcard');
    expect(rcards.length).toBe(5);

    // Motion path must NOT be mounted: no .wrapA, no .surface, no inline-height deck-track
    expect(container.querySelector('.wrapA')).toBeNull();
    expect(container.querySelector('.surface')).toBeNull();
    const track = container.querySelector('.deck-track') as HTMLElement | null;
    // In stacked mode the track has no inline height style
    expect(track?.style.height ?? '').toBe('');
  });
});
