import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup, within } from '@testing-library/react';
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

  it('opens the command palette from a ⌘K button', () => {
    render(<WritWindow />);
    fireEvent.click(screen.getAllByRole('button', { name: 'Open command palette' })[0]!);
    expect(screen.getByPlaceholderText('Type a command…')).toBeTruthy();
  });

  it('runs a transform from the palette and marks the buffer edited', () => {
    render(<WritWindow />);
    fireEvent.click(screen.getAllByRole('button', { name: 'Open command palette' })[0]!);
    fireEvent.click(screen.getByText('Trim Leading Whitespace'));
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
});
