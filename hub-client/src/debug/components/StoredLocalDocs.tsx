import { useEffect, useState } from 'react'
import { useRepo } from '@automerge/automerge-repo-react-hooks'
import {
  isValidAutomergeUrl,
  type AutomergeUrl,
} from '@automerge/automerge-repo'
import { listLocalStoredDocumentIds } from '../services/localStoredDocs'

interface Props {
  documents: AutomergeUrl[]
  onAdd: (docUrl: AutomergeUrl) => void
}

function toAutomergeUrl(docId: string): AutomergeUrl | null {
  if (isValidAutomergeUrl(docId)) return docId as AutomergeUrl
  const prefixed = `automerge:${docId}`
  if (isValidAutomergeUrl(prefixed)) return prefixed as AutomergeUrl
  return null
}

/**
 * Lists every Automerge document ID persisted to the shared IndexedDB
 * `automerge.documents` store. Only shown in local-storage mode, where
 * the Repo reads from the same store.
 */
export function StoredLocalDocs({ documents, onAdd }: Props) {
  const repo = useRepo()
  const [ids, setIds] = useState<string[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [reloadKey, setReloadKey] = useState(0)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    listLocalStoredDocumentIds()
      .then((result) => {
        if (cancelled) return
        setIds(result)
        setError(null)
      })
      .catch((e: unknown) => {
        if (cancelled) return
        setError(e instanceof Error ? e.message : String(e))
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [reloadKey])

  const subscribe = async (docId: string) => {
    const url = toAutomergeUrl(docId)
    if (!url) return
    if (documents.includes(url)) return
    setBusy(docId)
    try {
      await repo.find(url)
      onAdd(url)
    } finally {
      setBusy(null)
    }
  }

  return (
    <div className="quick-pick">
      <h2>Stored locally</h2>
      <p className="empty" style={{ fontStyle: 'normal' }}>
        Documents persisted to this browser's <code>automerge.documents</code>{' '}
        IndexedDB store. Read-only: the debug page never writes here.
      </p>

      {loading && <p className="empty">Reading local storage…</p>}
      {error && (
        <div className="error" role="alert">
          Local storage read failed: {error}
        </div>
      )}

      {!loading && !error && ids.length === 0 && (
        <p className="empty">No Automerge documents found in local storage.</p>
      )}

      {!loading && !error && ids.length > 0 && (
        <>
          <ul className="quick-pick-items">
            {ids.map((docId) => {
              const url = toAutomergeUrl(docId)
              const already = url ? documents.includes(url) : false
              return (
                <li key={docId}>
                  <button
                    className="quick-pick-btn"
                    onClick={() => void subscribe(docId)}
                    disabled={busy === docId || already || !url}
                    title={docId}
                  >
                    <span className="label">
                      {already ? '(subscribed)' : 'Open from disk'}
                    </span>
                    <code>{docId.slice(0, 24)}…</code>
                  </button>
                </li>
              )
            })}
          </ul>
          <button onClick={() => setReloadKey((k) => k + 1)} style={{ alignSelf: 'flex-start' }}>
            Reload list
          </button>
        </>
      )}
    </div>
  )
}
