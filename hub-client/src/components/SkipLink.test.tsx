/**
 * Tests for the SkipLink component (WCAG 2.4.1 Bypass Blocks).
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import SkipLink from './SkipLink';

afterEach(cleanup);

describe('SkipLink', () => {
  it('renders a link targeting the main content landmark', () => {
    render(<SkipLink />);
    const link = screen.getByRole('link', { name: 'Skip to main content' });
    expect(link.getAttribute('href')).toBe('#main-content');
  });
});
