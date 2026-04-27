import { useState } from 'react'
import { useRepo } from '@automerge/automerge-repo-react-hooks'
import { isValidAutomergeUrl, type AutomergeUrl } from '@automerge/automerge-repo'

interface Props {
  documents: AutomergeUrl[]
  onAdd: (docUrl: AutomergeUrl) => void
  onRemove: (docUrl: AutomergeUrl) => void
}

export function DocumentList({ documents, onAdd, onRemove }: Props) {
  const repo = useRepo()
  const [input, setInput] = useState('')
  const [error, setError] = useState<string>()
  const [loading, setLoading] = useState(false)

  const handleAdd = async () => {
    const trimmed = input.trim()
    if (!trimmed) return

    let docUrl: AutomergeUrl
    if (isValidAutomergeUrl(trimmed)) {
      docUrl = trimmed as AutomergeUrl
    } else if (isValidAutomergeUrl(`automerge:${trimmed}`)) {
      docUrl = `automerge:${trimmed}` as AutomergeUrl
    } else {
      setError('Invalid document ID. Enter a valid automerge URL or document ID.')
      return
    }

    if (documents.includes(docUrl)) {
      setError('Document already subscribed')
      return
    }

    setLoading(true)
    setError(undefined)

    try {
      await repo.find(docUrl)
      onAdd(docUrl)
      setInput('')
    } catch (e) {
      setError(`Failed to subscribe: ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setLoading(false)
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !loading) {
      void handleAdd()
    }
  }

  return (
    <div className="document-list">
      <h2>Subscribed Documents</h2>

      <div className="add-document">
        <input
          type="text"
          value={input}
          onChange={(e) => {
            setInput(e.target.value)
            setError(undefined)
          }}
          onKeyDown={handleKeyDown}
          placeholder="Document ID or automerge:…"
          disabled={loading}
        />
        <button onClick={() => void handleAdd()} disabled={loading || !input.trim()}>
          {loading ? '…' : 'Subscribe'}
        </button>
      </div>

      {error && <div className="error">{error}</div>}

      <ul className="document-items">
        {documents.map((docUrl) => (
          <li key={docUrl}>
            <code title={docUrl}>{docUrl.slice(0, 30)}…</code>
            <button
              onClick={() => onRemove(docUrl)}
              className="remove-btn"
              title="Remove subscription"
            >
              x
            </button>
          </li>
        ))}
      </ul>

      {documents.length === 0 && (
        <p className="empty">
          No documents subscribed. Enter an Automerge URL above to start receiving
          sync messages for that document.
        </p>
      )}
    </div>
  )
}
