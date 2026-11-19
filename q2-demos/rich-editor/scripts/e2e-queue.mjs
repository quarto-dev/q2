// Runs `playwright test` with the given arguments, queued across every checkout
// of the repo.
//
// Several agents often run the e2e suite at once, each in its own worktree.
// Playwright sizes its worker pool for a machine to itself, so concurrent runs
// overload the CPU: each runs much slower than it would alone, and slow tests
// time out. Queueing the runs gets each one done sooner.
//
// A run that's small (a spec file or two, a `-g` filter) shouldn't wait for a
// whole suite, though. So there are two lanes:
//
// - big runs take one lock, so only one runs at a time, with Playwright's own
//   worker count;
// - small runs take one of SMALL_SLOTS locks and run with at most
//   SMALL_WORKERS workers, alongside a big run. That oversubscribes the CPU a
//   little, not twice over.
//
// A run's size is the number of tests `playwright test --list` finds with the
// same arguments (it loads the specs, without starting a browser).
//
// Each lock is a directory in the git common dir (shared by all worktrees),
// created atomically with `mkdir`. It records its owner's pid, so a lock left
// behind by a run that was killed is taken over rather than waited on.
//
// Runs in different checkouts can now overlap, so each gets a free port for its
// dev server (`E2E_PORT`): on a shared port, Playwright would reuse the other
// checkout's server and test its app.

import { execFileSync, spawn } from 'node:child_process'
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const POLL_MS = 2000
// Runs of at most this many tests use the small lane. One spec file in every
// browser fits.
const SMALL_MAX_TESTS = 150
const SMALL_SLOTS = 2
const SMALL_WORKERS = 2

function lockRoot() {
  try {
    return execFileSync('git', ['rev-parse', '--path-format=absolute', '--git-common-dir'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim()
  } catch {
    return tmpdir()
  }
}

function isAlive(pid) {
  try {
    process.kill(pid, 0)
    return true
  } catch (e) {
    return e.code === 'EPERM'
  }
}

function readOwner(dir) {
  try {
    return JSON.parse(readFileSync(join(dir, 'owner.json'), 'utf8'))
  } catch {
    return null
  }
}

function tryTake(dir) {
  try {
    mkdirSync(dir)
  } catch (e) {
    if (e.code !== 'EEXIST') throw e
    const owner = readOwner(dir)
    if (owner && !isAlive(owner.pid)) {
      // The run that held the lock is gone; take it over.
      rmSync(dir, { recursive: true, force: true })
      return tryTake(dir)
    }
    // No owner file yet: another run is between `mkdir` and writing it.
    return false
  }
  writeFileSync(
    join(dir, 'owner.json'),
    JSON.stringify({ pid: process.pid, cwd: process.cwd(), started: new Date().toISOString() }),
  )
  return true
}

// Waits for any one of `dirs` and returns the one it took.
async function acquire(dirs) {
  let announced = false
  for (;;) {
    for (const dir of dirs) if (tryTake(dir)) return dir
    if (!announced) {
      const owners = dirs.map(readOwner).filter(Boolean)
      if (owners.length > 0) {
        console.error('Waiting for an e2e run to finish:')
        for (const o of owners) console.error(`  started ${o.started} in ${o.cwd} (pid ${o.pid})`)
        announced = true
      }
    }
    await new Promise((r) => setTimeout(r, POLL_MS))
  }
}

function release(dir) {
  if (readOwner(dir)?.pid === process.pid) rmSync(dir, { recursive: true, force: true })
}

// The number of tests the run would run, or Infinity when listing fails.
function countTests(args) {
  try {
    const out = execFileSync('playwright', ['test', '--list', ...args], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    })
    const m = out.match(/^Total: (\d+) tests?/m)
    return m ? Number(m[1]) : 0
  } catch {
    return Infinity
  }
}

function freePort() {
  return new Promise((resolve, reject) => {
    const server = createServer()
    server.unref()
    server.on('error', reject)
    server.listen(0, () => {
      const { port } = server.address()
      server.close(() => resolve(port))
    })
  })
}

const args = process.argv.slice(2)
const root = lockRoot()
const tests = countTests(args)
const small = tests <= SMALL_MAX_TESTS
const lanes = small
  ? Array.from({ length: SMALL_SLOTS }, (_, i) => join(root, `rich-editor-e2e-small-${i}.lock`))
  : [join(root, 'rich-editor-e2e.lock')]

const dir = await acquire(lanes)

const runArgs = [...args]
const hasWorkers = args.some((a) => /^(-j|--workers)(=|$)/.test(a))
if (small && !hasWorkers) runArgs.push(`--workers=${Math.max(1, Math.min(tests, SMALL_WORKERS))}`)
const env = { ...process.env, E2E_PORT: process.env.E2E_PORT ?? String(await freePort()) }

const child = spawn('playwright', ['test', ...runArgs], { stdio: 'inherit', env })
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.on(signal, () => child.kill(signal))
}
child.on('exit', (code, signal) => {
  release(dir)
  process.exit(code ?? (signal ? 1 : 0))
})
child.on('error', (e) => {
  release(dir)
  console.error(e)
  process.exit(1)
})
