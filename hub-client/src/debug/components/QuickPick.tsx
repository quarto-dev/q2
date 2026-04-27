import { useState } from 'react'
import { useRepo } from '@automerge/automerge-repo-react-hooks'
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

export function QuickPick({ documents, onAdd }: Props) {
  const { loading, projects, projectSetPointer, error } = useLocalProjects()
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

  const hasAny = projects.length > 0 || projectSetPointer !== null
  if (!hasAny && !error) return null

  return (
    <div className="quick-pick">
      <h2>On this device</h2>

      {error && (
        <div className="error" role="alert">
          Local storage read failed: {error}
        </div>
      )}

      {projectSetPointer && (
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
