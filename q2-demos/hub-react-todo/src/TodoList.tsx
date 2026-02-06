import type { TodoItem } from './astHelpers.ts'

interface TodoListProps {
  items: TodoItem[]
  onToggle?: (index: number) => void
}

export function TodoList({ items, onToggle }: TodoListProps) {
  if (items.length === 0) {
    return <p style={{ color: '#888' }}>No todo items found.</p>
  }

  return (
    <ul style={{ listStyle: 'none', padding: 0 }}>
      {items.map(item => (
        <li key={item.itemIndex} style={{ padding: '4px 0' }}>
          <label style={{ cursor: onToggle ? 'pointer' : 'default', display: 'flex', alignItems: 'center', gap: '8px' }}>
            <input
              type="checkbox"
              checked={item.checked}
              onChange={() => onToggle?.(item.itemIndex)}
              disabled={!onToggle}
              style={{ width: '18px', height: '18px' }}
            />
            <span style={{ textDecoration: item.checked ? 'line-through' : 'none', color: item.checked ? '#888' : 'inherit' }}>
              {item.label}
            </span>
          </label>
        </li>
      ))}
    </ul>
  )
}
