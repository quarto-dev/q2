import { useState } from 'react'
import {
  useDocument,
  useDocHandle,
  useRepo,
} from '@automerge/automerge-repo-react-hooks'
import {
  isValidAutomergeUrl,
  type AutomergeUrl,
} from '@automerge/automerge-repo'

interface Props {
  documents: AutomergeUrl[]
  onAdd: (docUrl: AutomergeUrl) => void
  onRemove: (docUrl: AutomergeUrl) => void
}

export function DocumentViewer({ documents, onAdd, onRemove }: Props) {
  return (
    <div className="document-viewer">
      <h2>Document Contents</h2>
      {documents.length === 0 ? (
        <p className="empty">Subscribe to documents to see their content here.</p>
      ) : (
        <div className="document-panels">
          {documents.map((docUrl) => (
            <DocumentPanel
              key={docUrl}
              docUrl={docUrl}
              subscribed={documents}
              onAdd={onAdd}
              onRemove={onRemove}
            />
          ))}
        </div>
      )}
    </div>
  )
}

/**
 * Detect the Quarto index-document shape: `{ files: Record<string, string> }`.
 * The map values are Automerge docIds (bs58) for per-file documents.
 */
function indexDocFiles(doc: unknown): Record<string, string> | null {
  if (!doc || typeof doc !== 'object') return null
  const files = (doc as Record<string, unknown>).files
  if (!files || typeof files !== 'object' || Array.isArray(files)) return null
  const entries = Object.entries(files as Record<string, unknown>)
  if (entries.length === 0) return null
  for (const [, v] of entries) {
    if (typeof v !== 'string') return null
  }
  return files as Record<string, string>
}

function toAutomergeUrl(docId: string): AutomergeUrl | null {
  if (isValidAutomergeUrl(docId)) return docId as AutomergeUrl
  const prefixed = `automerge:${docId}`
  if (isValidAutomergeUrl(prefixed)) return prefixed as AutomergeUrl
  return null
}

interface PanelProps {
  docUrl: AutomergeUrl
  subscribed: AutomergeUrl[]
  onAdd: (docUrl: AutomergeUrl) => void
  onRemove: (docUrl: AutomergeUrl) => void
}

function DocumentPanel({ docUrl, subscribed, onAdd, onRemove }: PanelProps) {
  const handle = useDocHandle<unknown>(docUrl)
  const [doc] = useDocument<unknown>(docUrl)
  const state = handle?.state ?? 'loading'
  const files = state === 'ready' ? indexDocFiles(doc) : null

  return (
    <div className={`document-panel ${state}`}>
      <div className="document-header">
        <h3 title={docUrl}>
          <code>{docUrl.slice(0, 40)}…</code>
        </h3>
        <span className={`state-badge ${state}`}>{state}</span>
        <button
          className="remove-btn"
          onClick={() => onRemove(docUrl)}
          title="Stop watching this document"
          aria-label="Stop watching this document"
        >
          x
        </button>
      </div>

      <div className="document-content">
        {state === 'ready' && doc !== undefined ? (
          <pre className="json-content">{JSON.stringify(doc, null, 2)}</pre>
        ) : state === 'unavailable' ? (
          <p className="unavailable">
            Document is unavailable. The sync server may not have this document, or
            the document ID may be invalid.
          </p>
        ) : (
          <p className="loading">Loading document…</p>
        )}
      </div>

      {files && (
        <IndexFilesSubscribe
          files={files}
          subscribed={subscribed}
          onAdd={onAdd}
        />
      )}
    </div>
  )
}

function IndexFilesSubscribe({
  files,
  subscribed,
  onAdd,
}: {
  files: Record<string, string>
  subscribed: AutomergeUrl[]
  onAdd: (docUrl: AutomergeUrl) => void
}) {
  const repo = useRepo()
  const [busy, setBusy] = useState<string | null>(null)
  const [err, setErr] = useState<string | null>(null)

  const subscribe = async (docId: string) => {
    const url = toAutomergeUrl(docId)
    if (!url) {
      setErr(`Not a valid document ID: ${docId}`)
      return
    }
    if (subscribed.includes(url)) return
    setBusy(docId)
    setErr(null)
    try {
      await repo.find(url)
      onAdd(url)
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(null)
    }
  }

  const entries = Object.entries(files).sort(([a], [b]) => a.localeCompare(b))

  return (
    <div className="index-files">
      <h4>Files in this index</h4>
      <ul className="index-files-list">
        {entries.map(([path, docId]) => {
          const url = toAutomergeUrl(docId)
          const alreadySubscribed = url ? subscribed.includes(url) : false
          return (
            <li key={path}>
              <span className="file-path" title={path}>
                {path}
              </span>
              <code title={docId}>{docId.slice(0, 16)}…</code>
              <button
                onClick={() => void subscribe(docId)}
                disabled={
                  busy === docId || alreadySubscribed || url === null
                }
                title={
                  url === null
                    ? 'Not a valid document ID'
                    : alreadySubscribed
                      ? 'Already subscribed'
                      : 'Subscribe to this file document'
                }
              >
                {alreadySubscribed ? 'subscribed' : busy === docId ? '…' : 'open'}
              </button>
            </li>
          )
        })}
      </ul>
      {err && <div className="error">{err}</div>}
    </div>
  )
}
