import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react';
import WritWindow from '../../WritWindow';

afterEach(cleanup);

function openPalette() {
  fireEvent.click(screen.getByLabelText('Open command palette'));
  return screen.getByPlaceholderText('Search commands');
}

describe('WritWindow', () => {
  it('mounts the CodeMirror editor on the default report.md buffer', () => {
    const { container } = render(<WritWindow />);
    expect(screen.getAllByTitle('report.md').length).toBeGreaterThan(0);
    expect(container.querySelector('.cm-editor')).toBeTruthy();
    expect(screen.getByText(/tok/)).toBeTruthy();
  });

  it('lists the four open buffers as tabs', () => {
    render(<WritWindow />);
    for (const name of ['report.md', 'settle.ts', 'schema.sql', 'gateway.log']) {
      expect(screen.getAllByTitle(name).length).toBeGreaterThan(0);
    }
  });

  it('opens the command palette from the status-bar cue', () => {
    render(<WritWindow />);
    expect(openPalette()).toBeTruthy();
  });

  it('opens the command palette on a double-tap of Shift when focused', () => {
    render(<WritWindow />);
    (screen.getByLabelText('Search buffers') as HTMLInputElement).focus();
    fireEvent.keyDown(document, { key: 'Shift' });
    fireEvent.keyDown(document, { key: 'Shift' });
    expect(screen.getByPlaceholderText('Search commands')).toBeTruthy();
  });

  it('lists a line-op command with a keycap and runs it through CodeMirror', async () => {
    const { container } = render(<WritWindow />);
    openPalette();
    const row = screen.getByText('Duplicate Line').closest('.wwx-pcmd') as HTMLElement;
    expect(row).toBeTruthy();
    expect(row.querySelector('.wwx-key')).toBeTruthy();
    fireEvent.click(screen.getByText('Duplicate Line'));
    // A CodeMirror edit surfaces through the update listener as the (debounced) save state.
    await waitFor(() => expect(container.querySelector('.wwx-save')).toBeTruthy(), { timeout: 2000 });
  });

  it('runs the real markdown bold command and shows FORMAT only for markdown', async () => {
    const { container } = render(<WritWindow />);
    openPalette();
    fireEvent.click(screen.getByText('Toggle Bold'));
    await waitFor(() => expect(container.querySelector('.wwx-save')).toBeTruthy(), { timeout: 2000 });

    fireEvent.click(screen.getAllByTitle('settle.ts')[0]!);
    openPalette();
    expect(screen.queryByText('Toggle Bold')).toBeNull();
    expect(screen.getByText('Duplicate Line')).toBeTruthy();
  });

  it('toggles a task checkbox in the preview by rewiring the CodeMirror document', () => {
    const { container } = render(<WritWindow />);
    const box = container.querySelector('[data-task="1"]') as HTMLElement;
    expect(box.getAttribute('aria-checked')).toBe('false');
    fireEvent.click(box);
    const after = container.querySelector('[data-task="1"]') as HTMLElement;
    expect(after.getAttribute('aria-checked')).toBe('true');
  });

  it('hides the preview toggle on a non-markdown buffer', () => {
    render(<WritWindow />);
    fireEvent.click(screen.getAllByTitle('settle.ts')[0]!);
    expect(screen.queryByTitle('Preview')).toBeNull();
  });

  it('searches across buffers and reports a result count', () => {
    render(<WritWindow />);
    fireEvent.change(screen.getByLabelText('Search buffers'), { target: { value: 'settle' } });
    expect(screen.getByText(/results?$/)).toBeTruthy();
    expect(screen.getByText('Results')).toBeTruthy();
  });

  it('runs spelling off by default, flags seeded misspellings, and fixes them', async () => {
    render(<WritWindow />);
    fireEvent.click(screen.getByText('notes.md'));
    const chip = await screen.findByRole('button', { name: 'Spelling off' });
    fireEvent.click(chip);
    fireEvent.click(screen.getByText('Turn on spelling'));
    const flagged = await screen.findByRole('button', { name: /\d+ spelling/ });
    expect(flagged).toBeTruthy();
    fireEvent.click(flagged);
    fireEvent.click(screen.getByText(/Fix all/));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Spelling' })).toBeTruthy(),
    );
  });
});
