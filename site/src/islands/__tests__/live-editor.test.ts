import { describe, it, expect, vi, beforeEach, type MockInstance } from 'vitest';

// Mock the wasm module before any import of the island logic
vi.mock('../../wasm/writ-render/writ_render.js', () => ({
  default: vi.fn().mockResolvedValue(undefined),
  render_fragment: vi.fn(),
}));

// Spy holders
let mermaidRunSpy: MockInstance;
let katexAutoRenderSpy: MockInstance;

vi.mock('mermaid', () => {
  mermaidRunSpy = vi.fn().mockResolvedValue(undefined);
  return {
    default: {
      initialize: vi.fn(),
      run: mermaidRunSpy,
    },
  };
});

vi.mock('katex/dist/contrib/auto-render', () => {
  katexAutoRenderSpy = vi.fn();
  return { default: katexAutoRenderSpy };
});

// Import after mocks are registered
import { runRenderPipeline } from '../LiveEditor.js';
import { render_fragment } from '../../wasm/writ-render/writ_render.js';

const mockRenderFragment = render_fragment as unknown as MockInstance;

function makeDocEl(): HTMLElement {
  const div = document.createElement('div');
  div.id = 'doc';
  document.body.appendChild(div);
  return div;
}

beforeEach(() => {
  document.body.innerHTML = '';
  vi.clearAllMocks();
});

describe('runRenderPipeline', () => {
  it('sets innerHTML when has_mermaid=false, has_math=false', async () => {
    mockRenderFragment.mockReturnValue({
      html: '<h1>x</h1>',
      has_mermaid: false,
      has_math: false,
    });
    const docEl = makeDocEl();
    await runRenderPipeline('# x', docEl);
    expect(docEl.innerHTML).toBe('<h1>x</h1>');
  });

  it('does NOT import mermaid or katex when flags are false', async () => {
    mockRenderFragment.mockReturnValue({
      html: '<p>plain</p>',
      has_mermaid: false,
      has_math: false,
    });
    const docEl = makeDocEl();
    await runRenderPipeline('plain', docEl);
    // Dynamic imports should not have been triggered
    const { default: mermaid } = await import('mermaid');
    expect(mermaid.run).not.toHaveBeenCalled();
    const { default: autoRender } = await import('katex/dist/contrib/auto-render');
    expect(autoRender).not.toHaveBeenCalled();
  });

  it('calls mermaid.run when has_mermaid=true', async () => {
    mockRenderFragment.mockReturnValue({
      html: '<pre class="mermaid">graph LR\nA-->B</pre>',
      has_mermaid: true,
      has_math: false,
    });
    const docEl = makeDocEl();
    docEl.innerHTML = '<pre class="mermaid">graph LR\nA-->B</pre>';
    await runRenderPipeline('```mermaid\ngraph LR\nA-->B\n```', docEl);
    const { default: mermaid } = await import('mermaid');
    expect(mermaid.run).toHaveBeenCalled();
  });

  it('calls renderMathInElement when has_math=true', async () => {
    mockRenderFragment.mockReturnValue({
      html: '<p>$$ x $$</p>',
      has_mermaid: false,
      has_math: true,
    });
    const docEl = makeDocEl();
    await runRenderPipeline('$$ x $$', docEl);
    const { default: autoRender } = await import('katex/dist/contrib/auto-render');
    expect(autoRender).toHaveBeenCalledWith(docEl, expect.objectContaining({ delimiters: expect.any(Array) }));
  });
});
