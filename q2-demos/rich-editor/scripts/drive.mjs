// Runs a one-off script (say, one that drives a mockup in Playwright) from
// anywhere, with this package's dependencies:
//
//   npm run drive -- /path/to/script.mjs [args...]
//
// Node resolves a bare import like `@playwright/test` from the importing
// file's folder, so a script outside the repo (an agent's scratchpad) can't
// find it, and scripts were being copied into the package to run. This
// registers a resolve hook that falls back to resolving from this package.
// The script sees its own arguments in `process.argv.slice(2)`.

import { mkdirSync } from 'node:fs'
import { register } from 'node:module'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const [file, ...rest] = process.argv.slice(2)
if (!file) {
  console.error('Usage: npm run drive -- <script.mjs> [args...]')
  process.exit(2)
}

register('./drive-hooks.mjs', import.meta.url)

// Firefox needs this to launch from some shells; see `playwright.config.ts`.
if (!process.env.CFFIXED_USER_HOME) {
  const home = join(tmpdir(), 'rich-editor-e2e-firefox-home')
  mkdirSync(home, { recursive: true })
  process.env.CFFIXED_USER_HOME = home
}

const script = resolve(file)
process.argv = [process.argv[0], script, ...rest]
await import(pathToFileURL(script).href)
