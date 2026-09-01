/**
 * Tests for BranchBar — the branch strip above the document editor.
 *
 * Pure presentational component: branch list + active id in, callbacks out.
 * Service behavior (fork/merge/persist) is covered in
 * services/branchService.test.ts.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import BranchBar from './BranchBar';
import type { BranchMeta } from '../services/branchService';

const branches: BranchMeta[] = [
  { id: 'b1', name: 'idea-1', createdAt: 1 },
  { id: 'b2', name: 'idea-2', createdAt: 2 },
];

function renderBar(overrides: Partial<Parameters<typeof BranchBar>[0]> = {}) {
  const props = {
    branches,
    activeBranchId: null as string | null,
    disabled: false,
    onSwitch: vi.fn(),
    onFork: vi.fn(),
    onMerge: vi.fn(),
    onDelete: vi.fn(),
    ...overrides,
  };
  render(<BranchBar {...props} />);
  return props;
}

afterEach(cleanup);

describe('BranchBar', () => {
  it('renders main plus one chip per branch, marking the active one', () => {
    renderBar({ activeBranchId: 'b2' });
    expect(screen.getByText('main')).toBeTruthy();
    expect(screen.getByText('idea-1')).toBeTruthy();
    const active = screen.getByText('idea-2').closest('button');
    expect(active?.className).toContain('active');
    const inactive = screen.getByText('main').closest('button');
    expect(inactive?.className).not.toContain('active');
  });

  it('clicking a branch chip switches to it; clicking main switches back', () => {
    const props = renderBar({ activeBranchId: 'b1' });
    fireEvent.click(screen.getByText('idea-2'));
    expect(props.onSwitch).toHaveBeenCalledWith('b2');
    fireEvent.click(screen.getByText('main'));
    expect(props.onSwitch).toHaveBeenCalledWith(null);
  });

  it('fork flow: click Fork, type a name, Enter confirms', () => {
    const props = renderBar();
    fireEvent.click(screen.getByText('Fork'));
    const input = screen.getByPlaceholderText(/branch name/i);
    fireEvent.change(input, { target: { value: 'my-fork' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(props.onFork).toHaveBeenCalledWith('my-fork');
  });

  it('fork flow: Escape cancels without forking', () => {
    const props = renderBar();
    fireEvent.click(screen.getByText('Fork'));
    const input = screen.getByPlaceholderText(/branch name/i);
    fireEvent.keyDown(input, { key: 'Escape' });
    expect(props.onFork).not.toHaveBeenCalled();
    expect(screen.queryByPlaceholderText(/branch name/i)).toBeNull();
  });

  it('shows Merge to main only when a branch is active, and wires it', () => {
    renderBar({ activeBranchId: null });
    expect(screen.queryByText(/merge to main/i)).toBeNull();
    cleanup();
    const props = renderBar({ activeBranchId: 'b1' });
    fireEvent.click(screen.getByText(/merge to main/i));
    expect(props.onMerge).toHaveBeenCalledWith('b1');
  });

  it('delete button on a chip deletes without switching', () => {
    const props = renderBar();
    const chip = screen.getByText('idea-1').closest('button')!;
    const del = chip.querySelector('.branch-chip-delete')!;
    fireEvent.click(del);
    expect(props.onDelete).toHaveBeenCalledWith('b1');
    expect(props.onSwitch).not.toHaveBeenCalled();
  });

  it('disabled mode disables all controls', () => {
    const props = renderBar({ disabled: true });
    fireEvent.click(screen.getByText('main'));
    fireEvent.click(screen.getByText('Fork'));
    expect(props.onSwitch).not.toHaveBeenCalled();
    expect(props.onFork).not.toHaveBeenCalled();
  });
});
