// Runs `playwright test` with the given arguments, one run at a time across
// every checkout of the repo.
//
// Several agents often run the e2e suite at once, each in its own worktree.
// Playwright sizes its worker pool for a machine to itself, so concurrent runs
// overload the CPU: each runs much slower than it would alone, and slow tests
// time out. Queueing the runs gets each one done sooner.
//
// The lock is a directory in the git common dir (shared by all worktrees),
// created atomically with `mkdir`. It records its owner's pid, so a lock left
// behind by a run that was killed is taken over rather than waited on.

import { execFileSync, spawn } from 'node:child_process'
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const POLL_MS = 2000

function lockDir() {
  try {
    const common = execFileSync('git', ['rev-parse', '--path-format=absolute', '--git-common-dir'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim()
    return join(common, 'rich-editor-e2e.lock')
  } catch {
    return join(tmpdir(), 'rich-editor-e2e.lock')
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

async function acquire(dir) {
  let announced = false
  for (;;) {
    try {
      mkdirSync(dir)
      writeFileSync(
        join(dir, 'owner.json'),
        JSON.stringify({ pid: process.pid, cwd: process.cwd(), started: new Date().toISOString() }),
      )
      return
    } catch (e) {
      if (e.code !== 'EEXIST') throw e
    }
    const owner = readOwner(dir)
    if (owner && !isAlive(owner.pid)) {
      // The run that held the lock is gone; take it over.
      rmSync(dir, { recursive: true, force: true })
      continue
    }
    if (!announced && owner) {
      console.error(
        `Waiting for the e2e run started ${owner.started} in ${owner.cwd} (pid ${owner.pid}) to finish...`,
      )
      announced = true
    }
    // No owner file yet: another run is between `mkdir` and writing it.
    await new Promise((r) => setTimeout(r, POLL_MS))
  }
}

function release(dir) {
  if (readOwner(dir)?.pid === process.pid) rmSync(dir, { recursive: true, force: true })
}

const dir = lockDir()
await acquire(dir)

const child = spawn('playwright', ['test', ...process.argv.slice(2)], { stdio: 'inherit' })
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
