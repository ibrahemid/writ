import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import WritWindow from '../../WritWindow';

afterEach(cleanup);

describe('WritWindow', () => {
  it('renders the default report.md window on mount', () => {
    render(<WritWindow />);
    expect(screen.getAllByTitle('report.md').length).toBeGreaterThan(0);
    expect(screen.getByText(/tok/)).toBeTruthy();
  });

  it('lists the four open buffers as tabs', () => {
    render(<WritWindow />);
    for (const name of ['report.md', 'settle.ts', 'schema.sql', 'gateway.log']) {
      expect(screen.getAllByTitle(name).length).toBeGreaterThan(0);
    }
  });

  it('opens the command palette from a ⇧⇧ button', () => {
    render(<WritWindow />);
    fireEvent.click(screen.getAllByRole('button', { name: 'Open command palette (Shift Shift)' })[0]!);
    expect(screen.getByPlaceholderText('Type a command…')).toBeTruthy();
  });

  it('opens the command palette on a double-tap of Shift when focused', () => {
    render(<WritWindow />);
    const input = screen.getByPlaceholderText('Search buffers…') as HTMLInputElement;
    input.focus();
    fireEvent.keyDown(document, { key: 'Shift' });
    fireEvent.keyDown(document, { key: 'Shift' });
    expect(screen.getByPlaceholderText('Type a command…')).toBeTruthy();
  });

  it('does not open the palette on a single Shift', () => {
    render(<WritWindow />);
    const input = screen.getByPlaceholderText('Search buffers…') as HTMLInputElement;
    input.focus();
    fireEvent.keyDown(document, { key: 'Shift' });
    expect(screen.queryByPlaceholderText('Type a command…')).toBeNull();
  });

  it('runs a transform from the palette and marks the buffer edited', () => {
    render(<WritWindow />);
    fireEvent.click(screen.getAllByRole('button', { name: 'Open command palette (Shift Shift)' })[0]!);
    fireEvent.click(screen.getByText('Trim leading spaces'));
    expect(screen.getByText('Edited')).toBeTruthy();
  });

  it('searches and lands a matching line', () => {
    render(<WritWindow />);
    const input = screen.getByPlaceholderText('Search buffers…');
    fireEvent.change(input, { target: { value: 'settle' } });
    expect(screen.getByText(/hit/)).toBeTruthy();
  });

  it('switches to a non-markdown buffer and hides the preview toggle', () => {
    render(<WritWindow />);
    fireEvent.click(screen.getAllByTitle('settle.ts')[0]!);
    expect(screen.queryByText('Preview')).toBeNull();
  });

  it('toggles a task checkbox in the preview and marks the buffer edited', () => {
    const { container } = render(<WritWindow />);
    const box = container.querySelector('[data-task="1"]') as HTMLElement;
    expect(box).toBeTruthy();
    expect(box.getAttribute('aria-checked')).toBe('false');
    fireEvent.click(box);
    const after = container.querySelector('[data-task="1"]') as HTMLElement;
    expect(after.getAttribute('aria-checked')).toBe('true');
    expect(screen.getByText('Edited')).toBeTruthy();
  });

  it('wraps the selection in bold on Cmd+B', () => {
    render(<WritWindow />);
    const ta = screen.getByLabelText('Markdown source, editable') as HTMLTextAreaElement;
    ta.focus();
    ta.setSelectionRange(2, 10);
    fireEvent.keyDown(ta, { key: 'b', metaKey: true });
    expect(ta.value).toContain('**');
    expect(screen.getByText('Edited')).toBeTruthy();
  });

  it('shows the FORMAT group for a markdown buffer and hides it otherwise', () => {
    render(<WritWindow />);
    fireEvent.click(screen.getAllByRole('button', { name: 'Open command palette (Shift Shift)' })[0]!);
    expect(screen.getByText('Bold')).toBeTruthy();
    fireEvent.keyDown(document, { key: 'Escape' });
    fireEvent.click(screen.getAllByTitle('settle.ts')[0]!);
    fireEvent.click(screen.getAllByRole('button', { name: 'Open command palette (Shift Shift)' })[0]!);
    expect(screen.queryByText('Bold')).toBeNull();
  });
});
