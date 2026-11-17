/**
 * @vitest-environment jsdom
 */

import { readFileSync } from 'node:fs';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import Tooltip from './Tooltip';

const styles = readFileSync('src/components/Tooltip.css', 'utf8');

describe('Tooltip', () => {
  afterEach(cleanup);

  it('wraps long unbroken content inside its bounded bubble', () => {
    const content = 'A'.repeat(100);
    render(
      <Tooltip content={content}>
        <span tabIndex={0}>Anchor</span>
      </Tooltip>,
    );

    fireEvent.focus(screen.getByText('Anchor'));
    expect(screen.getByRole('tooltip').textContent).toBe(content);
    expect(styles).toMatch(
      /\.qh-tooltip\s*\{[^}]*box-sizing:\s*border-box;[^}]*max-width:\s*min\(300px, calc\(100vw - 8px\)\);[^}]*overflow-wrap:\s*anywhere;/s,
    );
  });
});
