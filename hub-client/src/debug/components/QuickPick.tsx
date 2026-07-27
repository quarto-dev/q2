import { useState } from 'react'
import { useRepo, useDocument } from '@automerge/automerge-repo-react-hooks'
import {
  isValidAutomergeUrl,
  type AutomergeUrl,
} from '@automerge/automerge-repo'
import { useLocalProjects } from '../hooks/useLocalProjects'

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
 * Label for a collection quick-pick card: the collection's own name (from its
 * loaded ProjectSetDocument) with a `(root)` tag for the personal root set.
 * Falls back to a generic label while the doc is still loading or when it has
 * no name.
 */
export function collectionLabel(doc: unknown, isRoot: boolean): string {
  const rawName =
    doc && typeof doc === 'object'
      ? (doc as { name?: unknown }).name
      : undefined
  const name = typeof rawName === 'string' && rawName.trim() ? rawName : null
  const base = name ?? 'Collection'
  return isRoot ? `${base} (root)` : base
}

interface CollectionItemProps {
  url: AutomergeUrl
  rawDocId: string
  isRoot: boolean
  subscribing: string | null
  onSubscribe: (rawDocId: string) => void
}

/**
 * One collection card. Reads the collection's ProjectSetDocument via the repo
 * so the card can show its real name; updates reactively once it syncs.
 */
function CollectionItem({ url, rawDocId, isRoot, subscribing, onSubscribe }: CollectionItemProps) {
  const [doc] = useDocument<{ name?: string }>(url)
  return (
    <li>
      <button
        className="quick-pick-btn"
        onClick={() => onSubscribe(rawDocId)}
        disabled={subscribing === rawDocId}
        title={rawDocId}
      >
        <span className="label">{collectionLabel(doc, isRoot)}</span>
        <code>{rawDocId.slice(0, 20)}…</code>
      </button>
    </li>
  )
}

const stripPrefix = (id: string) => id.replace(/^automerge:/, '')

export function QuickPick({ documents, onAdd }: Props) {
  const { loading, projects, projectSetPointer, collectionPointers, error } =
    useLocalProjects()
  const repo = useRepo()
  const [subscribing, setSubscribing] = useState<string | null>(null)
  const [subscribeError, setSubscribeError] = useState<string | null>(null)

  const subscribe = async (rawDocId: string) => {
    const url = toAutomergeUrl(rawDocId)
    if (!url) {
      setSubscribeError(`Not a valid Automerge URL: ${rawDocId}`)
      return
    }
    if (documents.includes(url)) {
      setSubscribeError('Already subscribed to that document.')
      return
    }
    setSubscribing(rawDocId)
    setSubscribeError(null)
    try {
      await repo.find(url)
      onAdd(url)
    } catch (e) {
      setSubscribeError(e instanceof Error ? e.message : String(e))
    } finally {
      setSubscribing(null)
    }
  }

  if (loading) return null

  const hasAny =
    projects.length > 0 ||
    projectSetPointer !== null ||
    collectionPointers.length > 0
  if (!hasAny && !error) return null

  // The root collection is the one whose doc id matches the legacy singleton
  // pointer (identity, not array position — the stored order can be scrambled).
  const rootDocId = projectSetPointer
    ? stripPrefix(projectSetPointer.projectSetDocId)
    : null

  return (
    <div className="quick-pick">
      <h2>On this device</h2>

      {error && (
        <div className="error" role="alert">
          Local storage read failed: {error}
        </div>
      )}

      {collectionPointers.length > 0 ? (
        <div className="quick-pick-group">
          <h3>Collections</h3>
          <ul className="quick-pick-items">
            {collectionPointers.map((c) => {
              const isRoot = rootDocId !== null && stripPrefix(c.projectSetDocId) === rootDocId
              const url = toAutomergeUrl(c.projectSetDocId)
              // Invalid doc id: no name lookup possible, render a plain card.
              if (!url) {
                return (
                  <li key={c.projectSetDocId}>
                    <button
                      className="quick-pick-btn"
                      onClick={() => void subscribe(c.projectSetDocId)}
                      disabled={subscribing === c.projectSetDocId}
                      title={c.projectSetDocId}
                    >
                      <span className="label">{collectionLabel(undefined, isRoot)}</span>
                      <code>{c.projectSetDocId.slice(0, 20)}…</code>
                    </button>
                  </li>
                )
              }
              return (
                <CollectionItem
                  key={c.projectSetDocId}
                  url={url}
                  rawDocId={c.projectSetDocId}
                  isRoot={isRoot}
                  subscribing={subscribing}
                  onSubscribe={(id) => void subscribe(id)}
                />
              )
            })}
          </ul>
        </div>
      ) : (
        projectSetPointer && (
          <div className="quick-pick-group">
            <h3>Project set</h3>
            <ul className="quick-pick-items">
              <li>
                <button
                  className="quick-pick-btn"
                  onClick={() => void subscribe(projectSetPointer.projectSetDocId)}
                  disabled={subscribing === projectSetPointer.projectSetDocId}
                  title={projectSetPointer.projectSetDocId}
                >
                  <span className="label">Project set document</span>
                  <code>{projectSetPointer.projectSetDocId.slice(0, 20)}…</code>
                </button>
              </li>
            </ul>
          </div>
        )
      )}

      {projects.length > 0 && (
        <div className="quick-pick-group">
          <h3>Projects</h3>
          <ul className="quick-pick-items">
            {projects.map((p) => (
              <li key={p.id}>
                <button
                  className="quick-pick-btn"
                  onClick={() => void subscribe(p.indexDocId)}
                  disabled={subscribing === p.indexDocId}
                  title={p.indexDocId}
                >
                  <span className="label">
                    {p.description || '(no description)'}
                  </span>
                  <code>{p.indexDocId.slice(0, 20)}…</code>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {subscribeError && <div className="error">{subscribeError}</div>}
    </div>
  )
}
