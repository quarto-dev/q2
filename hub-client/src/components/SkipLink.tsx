/**
 * Skip Link
 *
 * "Skip to main content" link — the first focusable element in the app
 * (WCAG 2.4.1 Bypass Blocks). Visually hidden until focused. Targets
 * the `#main-content` landmark rendered by the active view (Editor or
 * ProjectsHome).
 */

import './SkipLink.css';

export default function SkipLink() {
  return (
    <a href="#main-content" className="skip-link">
      Skip to main content
    </a>
  );
}
