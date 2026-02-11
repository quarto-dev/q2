/**
 * Calendar view: shows cards with deadlines in a month grid.
 */

import { useState } from 'react'
import type { KanbanCard } from '../types.ts'

interface CalendarViewProps {
  cards: KanbanCard[]
  onCardClick?: (card: KanbanCard) => void
}

const TYPE_COLORS: Record<string, string> = {
  feature: '#e3f2fd',
  milestone: '#fff3e0',
  bug: '#fce4ec',
  task: '#e8f5e9',
}

const DAY_NAMES = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']
const MONTH_NAMES = [
  'January', 'February', 'March', 'April', 'May', 'June',
  'July', 'August', 'September', 'October', 'November', 'December',
]

/**
 * Parse a deadline string (YYYY-MM-DD or ISO date) to a date-only string YYYY-MM-DD.
 */
function parseDeadlineDate(deadline: string): string | null {
  // Extract just the date portion
  const match = deadline.match(/^(\d{4}-\d{2}-\d{2})/)
  return match ? match[1] : null
}

/**
 * Get the days in a month, including padding days from previous/next months
 * to fill complete weeks.
 */
function getCalendarDays(year: number, month: number): { date: Date; inMonth: boolean }[] {
  const firstDay = new Date(year, month, 1)
  const lastDay = new Date(year, month + 1, 0)
  const days: { date: Date; inMonth: boolean }[] = []

  // Pad with days from previous month
  const startDayOfWeek = firstDay.getDay()
  for (let i = startDayOfWeek - 1; i >= 0; i--) {
    const d = new Date(year, month, -i)
    days.push({ date: d, inMonth: false })
  }

  // Days in current month
  for (let d = 1; d <= lastDay.getDate(); d++) {
    days.push({ date: new Date(year, month, d), inMonth: true })
  }

  // Pad with days from next month to complete the last week
  const remaining = 7 - (days.length % 7)
  if (remaining < 7) {
    for (let d = 1; d <= remaining; d++) {
      days.push({ date: new Date(year, month + 1, d), inMonth: false })
    }
  }

  return days
}

function formatDateKey(date: Date): string {
  const y = date.getFullYear()
  const m = String(date.getMonth() + 1).padStart(2, '0')
  const d = String(date.getDate()).padStart(2, '0')
  return `${y}-${m}-${d}`
}

export function CalendarView({ cards, onCardClick }: CalendarViewProps) {
  const today = new Date()
  const [year, setYear] = useState(today.getFullYear())
  const [month, setMonth] = useState(today.getMonth())

  // Build a map from date string to cards with deadlines on that date
  const cardsByDate = new Map<string, KanbanCard[]>()
  for (const card of cards) {
    if (!card.deadline) continue
    const dateStr = parseDeadlineDate(card.deadline)
    if (!dateStr) continue
    const existing = cardsByDate.get(dateStr)
    if (existing) {
      existing.push(card)
    } else {
      cardsByDate.set(dateStr, [card])
    }
  }

  const calendarDays = getCalendarDays(year, month)
  const todayKey = formatDateKey(today)

  const prevMonth = () => {
    if (month === 0) {
      setMonth(11)
      setYear(year - 1)
    } else {
      setMonth(month - 1)
    }
  }

  const nextMonth = () => {
    if (month === 11) {
      setMonth(0)
      setYear(year + 1)
    } else {
      setMonth(month + 1)
    }
  }

  const goToToday = () => {
    setYear(today.getFullYear())
    setMonth(today.getMonth())
  }

  return (
    <div>
      {/* Month navigation */}
      <div style={{
        display: 'flex',
        alignItems: 'center',
        gap: '12px',
        marginBottom: '16px',
      }}>
        <button onClick={prevMonth} style={navButtonStyle}>&larr;</button>
        <h3 style={{ margin: 0, fontSize: '16px', minWidth: '180px', textAlign: 'center' }}>
          {MONTH_NAMES[month]} {year}
        </h3>
        <button onClick={nextMonth} style={navButtonStyle}>&rarr;</button>
        <button onClick={goToToday} style={{ ...navButtonStyle, fontSize: '12px', marginLeft: '8px' }}>
          Today
        </button>
      </div>

      {/* Day-of-week headers */}
      <div style={{
        display: 'grid',
        gridTemplateColumns: 'repeat(7, 1fr)',
        gap: '1px',
        background: '#ddd',
        border: '1px solid #ddd',
        borderRadius: '8px',
        overflow: 'hidden',
      }}>
        {DAY_NAMES.map(name => (
          <div key={name} style={{
            background: '#f0f0f0',
            padding: '6px 8px',
            fontSize: '12px',
            fontWeight: 600,
            textAlign: 'center',
            color: '#666',
          }}>
            {name}
          </div>
        ))}

        {/* Calendar cells */}
        {calendarDays.map(({ date, inMonth }) => {
          const key = formatDateKey(date)
          const dayCards = cardsByDate.get(key) ?? []
          const isToday = key === todayKey

          return (
            <div key={key} style={{
              background: isToday ? '#fffde7' : inMonth ? '#fff' : '#fafafa',
              padding: '4px',
              minHeight: '80px',
              verticalAlign: 'top',
            }}>
              <div style={{
                fontSize: '12px',
                fontWeight: isToday ? 700 : 400,
                color: inMonth ? (isToday ? '#1565c0' : '#333') : '#bbb',
                marginBottom: '4px',
                textAlign: 'right',
                padding: '2px 4px',
              }}>
                {date.getDate()}
              </div>
              {dayCards.map(card => (
                <div
                  key={card.id}
                  onClick={() => onCardClick?.(card)}
                  style={{
                    fontSize: '11px',
                    padding: '2px 4px',
                    marginBottom: '2px',
                    borderRadius: '3px',
                    background: card.type ? TYPE_COLORS[card.type] ?? '#f0f0f0' : '#f0f0f0',
                    cursor: onCardClick ? 'pointer' : 'default',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                  title={card.title}
                >
                  {card.title}
                </div>
              ))}
            </div>
          )
        })}
      </div>

      {/* Summary of cards without deadlines */}
      {(() => {
        const noDeadline = cards.filter(c => !c.deadline)
        if (noDeadline.length === 0) return null
        return (
          <p style={{ fontSize: '12px', color: '#888', marginTop: '12px' }}>
            {noDeadline.length} card{noDeadline.length !== 1 ? 's' : ''} without deadlines not shown.
          </p>
        )
      })()}
    </div>
  )
}

const navButtonStyle: React.CSSProperties = {
  background: 'none',
  border: '1px solid #ccc',
  borderRadius: '4px',
  padding: '4px 10px',
  cursor: 'pointer',
  fontSize: '14px',
}
