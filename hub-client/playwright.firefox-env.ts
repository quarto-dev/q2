import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// macOS 27 puts ~/Library/Application Support/Firefox behind TCC, and
// Playwright's bundled Firefox resolves that directory even though it is given
// a temp -profile. With a stock Firefox installed, every launch then hangs
// until the launch timeout (microsoft/playwright#42768). Pointing
// CFFIXED_USER_HOME at a fresh empty directory keeps Firefox away from it.
// Darwin only: Linux CI doesn't have the problem. Drop this once Playwright
// bundles Firefox 158+.
//
// Shared by every Playwright config that has a `firefox` project.
export const firefoxLaunchEnv: Record<string, string> =
  process.platform === 'darwin'
    ? {
        ...(process.env as Record<string, string>),
        CFFIXED_USER_HOME: mkdtempSync(join(tmpdir(), 'pw-firefox-home-')),
      }
    : (process.env as Record<string, string>);
