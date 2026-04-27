import { useState, useCallback, useEffect, useRef } from 'react'
import { RepoContext } from '@automerge/automerge-repo-react-hooks'
import {
  isValidAutomergeUrl,
  type AutomergeUrl,
  type Repo,
} from '@automerge/automerge-repo'
import { ConnectionStatus } from './components/ConnectionStatus'
import { DocumentList } from './components/DocumentList'
import { DocumentViewer } from './components/DocumentViewer'
import { MessageLog } from './components/MessageLog'
import { QuickPick } from './components/QuickPick'
import { StoredLocalDocs } from './components/StoredLocalDocs'
import { useMessageLog } from './hooks/useMessageLog'
import {
  createInspectorRepo,
  createLocalStorageRepo,
  defaultSyncServerUrl,
} from './services/repo'
import type { ConnectionInfo } from './types/messages'
import { useDebugAuthGate } from './hooks/useDebugAuthGate'
import { parseDebugHashSeed } from './utils/hashSeed'

type StorageMode = 'server' | 'local'

function GateShell({ children }: { children: React.ReactNode }) {
  return (
    <div className="debug-gate">
      <h1>Quarto Hub — Automerge Debugger</h1>
      {children}
    </div>
  )
}

function CheckingView() {
  return (
    <GateShell>
      <p>Checking sign-in status…</p>
    </GateShell>
  )
}

function AnonView() {
  return (
    <GateShell>
      <p>You need to sign in to Quarto Hub before using the debugger.</p>
      <p>
        <a href="./">Go to Quarto Hub and sign in</a>, then return to this page.
      </p>
    </GateShell>
  )
}

function UnverifiedBanner({ reason }: { reason: string }) {
  return (
    <div className="unverified-banner" role="status">
      <strong>Sign-in not verified.</strong>{' '}
      <code>/auth/me</code> did not respond (<code>{reason}</code>). That's
      expected for hub deployments without authentication; proceeding to the
      inspector. If the sync server requires auth you'll see a disconnect.
    </div>
  )
}

function ModeToggle({
  mode,
  onChange,
}: {
  mode: StorageMode
  onChange: (next: StorageMode) => void
}) {
  return (
    <div className="mode-toggle" role="tablist" aria-label="Storage source">
      <button
        className={mode === 'server' ? 'active' : ''}
        onClick={() => onChange('server')}
        role="tab"
        aria-selected={mode === 'server'}
      >
        Server (live)
      </button>
      <button
        className={mode === 'local' ? 'active' : ''}
        onClick={() => onChange('local')}
        role="tab"
        aria-selected={mode === 'local'}
      >
        Local IndexedDB
      </button>
    </div>
  )
}

function LocalStorageBanner({ onReload }: { onReload: () => void }) {
  return (
    <div className="local-banner">
      <span className="label">Local IndexedDB (read-only, never writes)</span>
      <button onClick={onReload}>Reload from disk</button>
    </div>
  )
}

function toAutomergeUrlOrNull(raw: string): AutomergeUrl | null {
  if (isValidAutomergeUrl(raw)) return raw as AutomergeUrl
  const prefixed = `automerge:${raw}`
  if (isValidAutomergeUrl(prefixed)) return prefixed as AutomergeUrl
  return null
}

function Inspector({ authNotice }: { authNotice?: string }) {
  const [mode, setMode] = useState<StorageMode>('server')
  const [syncServerUrl, setSyncServerUrl] = useState(defaultSyncServerUrl)
  const [connection, setConnection] = useState<ConnectionInfo>({
    state: 'disconnected',
  })
  const [subscribedDocs, setSubscribedDocs] = useState<AutomergeUrl[]>([])
  const { messages, addMessage, clearMessages } = useMessageLog()

  const repoRef = useRef<{ repo: Repo; shutdown: () => void } | null>(null)
  const [repo, setRepo] = useState<Repo | null>(null)

  const buildServerRepo = useCallback(() => {
    const inspector = createInspectorRepo({
      syncServerUrl,
      onMessage: addMessage,
      onConnectionChange: (connected, remotePeerId) => {
        setConnection((prev) => ({
          ...prev,
          state: connected ? 'connected' : 'disconnected',
          peerId: inspector.repo.peerId,
          remotePeerId,
        }))
      },
    })
    setConnection({
      state: 'connecting',
      peerId: inspector.repo.peerId,
      remotePeerId: undefined,
    })
    return inspector
  }, [syncServerUrl, addMessage])

  const rebuild = useCallback(() => {
    repoRef.current?.shutdown()
    repoRef.current = null

    const next =
      mode === 'server' ? buildServerRepo() : createLocalStorageRepo()
    repoRef.current = next
    setRepo(next.repo)
  }, [mode, buildServerRepo])

  // Build / rebuild the Repo whenever the mode changes.
  useEffect(() => {
    rebuild()
    return () => {
      repoRef.current?.shutdown()
      repoRef.current = null
    }
    // `rebuild` captures mode; reloading on mode change is the entire
    // point of this effect.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode])

  // One-shot hash seed: if the page was opened as debug.html#doc=<url>,
  // subscribe to that document once the repo is ready. Ignored on reload
  // and on mode changes (seed applies only to the very first mount).
  const hashSeedApplied = useRef(false)
  useEffect(() => {
    if (!repo || hashSeedApplied.current) return
    hashSeedApplied.current = true
    const seed = parseDebugHashSeed(window.location.hash)
    if (!seed) return
    const url = toAutomergeUrlOrNull(seed)
    if (!url) return
    void repo.find(url).then(
      () =>
        setSubscribedDocs((prev) => (prev.includes(url) ? prev : [...prev, url])),
      () => {
        /* Seed failures are non-fatal; user can subscribe manually. */
      },
    )
  }, [repo])

  const handleReconnect = () => {
    setSubscribedDocs([])
    clearMessages()
    rebuild()
  }

  const handleModeChange = (next: StorageMode) => {
    if (next === mode) return
    setSubscribedDocs([])
    clearMessages()
    setMode(next)
  }

  const handleAdd = (url: AutomergeUrl) => {
    setSubscribedDocs((prev) => (prev.includes(url) ? prev : [...prev, url]))
  }

  if (!repo) {
    return (
      <div className="debug-app loading">
        <h1>Automerge Debugger</h1>
        <p>Initializing…</p>
      </div>
    )
  }

  return (
    <RepoContext.Provider value={repo}>
      <div className="debug-app">
        <header className="app-header">
          <h1>Quarto Hub — Automerge Debugger</h1>
          <ModeToggle mode={mode} onChange={handleModeChange} />
          {mode === 'server' ? (
            <ConnectionStatus
              url={syncServerUrl}
              connection={connection}
              onUrlChange={setSyncServerUrl}
              onReconnect={handleReconnect}
            />
          ) : (
            <LocalStorageBanner onReload={handleReconnect} />
          )}
        </header>
        {authNotice && <UnverifiedBanner reason={authNotice} />}

        <main
          className={`app-main${mode === 'local' ? ' no-log-panel' : ''}`}
        >
          <aside className="sidebar">
            <DocumentList
              documents={subscribedDocs}
              onAdd={handleAdd}
              onRemove={(url) =>
                setSubscribedDocs((prev) => prev.filter((d) => d !== url))
              }
            />
            <QuickPick documents={subscribedDocs} onAdd={handleAdd} />
            {mode === 'local' && (
              <StoredLocalDocs
                documents={subscribedDocs}
                onAdd={handleAdd}
              />
            )}
          </aside>

          <section className="content">
            <DocumentViewer
              documents={subscribedDocs}
              onAdd={handleAdd}
              onRemove={(url) =>
                setSubscribedDocs((prev) => prev.filter((d) => d !== url))
              }
            />
          </section>

          {mode === 'server' && (
            <section className="log-panel">
              <MessageLog messages={messages} onClear={clearMessages} />
            </section>
          )}
        </main>
      </div>
    </RepoContext.Provider>
  )
}

export function DebugApp() {
  const gate = useDebugAuthGate()
  switch (gate.state) {
    case 'checking':
      return <CheckingView />
    case 'anon':
      return <AnonView />
    case 'authed':
      return <Inspector />
    case 'unverified':
      return <Inspector authNotice={gate.reason} />
  }
}
