/**
 * Modal form for creating a new kanban card.
 */

import { useState } from 'react'
import type { CardType, CardStatus } from '../types.ts'

export interface NewCardData {
  title: string
  type?: CardType
  status?: CardStatus
  deadline?: string
}

interface NewCardFormProps {
  onSubmit: (data: NewCardData) => void
  onClose: () => void
}

const CARD_TYPES: { value: CardType; label: string }[] = [
  { value: 'feature', label: 'Feature' },
  { value: 'milestone', label: 'Milestone' },
  { value: 'bug', label: 'Bug' },
  { value: 'task', label: 'Task' },
]

const CARD_STATUSES: { value: CardStatus; label: string }[] = [
  { value: 'todo', label: 'Todo' },
  { value: 'doing', label: 'Doing' },
  { value: 'done', label: 'Done' },
]

export function NewCardForm({ onSubmit, onClose }: NewCardFormProps) {
  const [title, setTitle] = useState('')
  const [type, setType] = useState<CardType | ''>('')
  const [status, setStatus] = useState<CardStatus | ''>('')
  const [deadline, setDeadline] = useState('')

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!title.trim()) return
    onSubmit({
      title: title.trim(),
      type: type || undefined,
      status: status || undefined,
      deadline: deadline || undefined,
    })
  }

  return (
    <div
      style={{
        position: 'fixed',
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        background: 'rgba(0, 0, 0, 0.5)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 1000,
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div style={{
        background: '#fff',
        borderRadius: '8px',
        padding: '24px',
        maxWidth: '480px',
        width: '90%',
        boxShadow: '0 4px 24px rgba(0, 0, 0, 0.2)',
      }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '20px' }}>
          <h2 style={{ margin: 0, fontSize: '18px' }}>New Card</h2>
          <button
            onClick={onClose}
            style={{
              background: 'none',
              border: 'none',
              fontSize: '20px',
              cursor: 'pointer',
              color: '#666',
              padding: '0 4px',
              lineHeight: 1,
            }}
          >
            &times;
          </button>
        </div>

        <form onSubmit={handleSubmit}>
          {/* Title */}
          <div style={{ marginBottom: '16px' }}>
            <label style={labelStyle}>Title *</label>
            <input
              type="text"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="Card title"
              autoFocus
              style={inputStyle}
            />
          </div>

          {/* Type */}
          <div style={{ marginBottom: '16px' }}>
            <label style={labelStyle}>Type</label>
            <div style={{ display: 'flex', gap: '8px', flexWrap: 'wrap' }}>
              {CARD_TYPES.map(ct => (
                <button
                  key={ct.value}
                  type="button"
                  onClick={() => setType(type === ct.value ? '' : ct.value)}
                  style={{
                    padding: '4px 12px',
                    borderRadius: '4px',
                    border: type === ct.value ? '2px solid #2563eb' : '1px solid #ccc',
                    background: type === ct.value ? '#e3f2fd' : '#fff',
                    cursor: 'pointer',
                    fontSize: '13px',
                  }}
                >
                  {ct.label}
                </button>
              ))}
            </div>
          </div>

          {/* Status */}
          <div style={{ marginBottom: '16px' }}>
            <label style={labelStyle}>Status</label>
            <select
              value={status}
              onChange={(e) => setStatus(e.target.value as CardStatus | '')}
              style={inputStyle}
            >
              <option value="">— None —</option>
              {CARD_STATUSES.map(cs => (
                <option key={cs.value} value={cs.value}>{cs.label}</option>
              ))}
            </select>
          </div>

          {/* Deadline */}
          <div style={{ marginBottom: '20px' }}>
            <label style={labelStyle}>Deadline</label>
            <input
              type="date"
              value={deadline}
              onChange={(e) => setDeadline(e.target.value)}
              style={inputStyle}
            />
          </div>

          {/* Actions */}
          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '8px' }}>
            <button
              type="button"
              onClick={onClose}
              style={{
                padding: '8px 16px',
                border: '1px solid #ccc',
                borderRadius: '4px',
                background: 'none',
                cursor: 'pointer',
                fontSize: '14px',
              }}
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={!title.trim()}
              style={{
                padding: '8px 16px',
                border: 'none',
                borderRadius: '4px',
                background: title.trim() ? '#2563eb' : '#ccc',
                color: '#fff',
                cursor: title.trim() ? 'pointer' : 'default',
                fontSize: '14px',
              }}
            >
              Create
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}

const labelStyle: React.CSSProperties = {
  display: 'block',
  fontSize: '13px',
  fontWeight: 600,
  color: '#555',
  marginBottom: '4px',
}

const inputStyle: React.CSSProperties = {
  width: '100%',
  padding: '8px',
  border: '1px solid #ccc',
  borderRadius: '4px',
  fontSize: '14px',
  boxSizing: 'border-box',
}
