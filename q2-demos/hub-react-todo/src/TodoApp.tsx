/**
 * Main todo app component.
 * Connects AST from useSyncedAst to TodoList via astHelpers.
 */

import type { RustQmdJson } from '@quarto/pandoc-types'
import { useSyncedAst } from './useSyncedAst.ts'
import { findTodoDiv, extractTodoItems } from './astHelpers.ts'
import { TodoList } from './TodoList.tsx'
import { useMemo } from 'react'

interface TodoAppProps {
  syncServer: string
  indexDocId: string
  filePath: string
}

export function TodoApp({ syncServer, indexDocId, filePath }: TodoAppProps) {
  const params = useMemo(
    () => ({ syncServer, indexDocId, filePath }),
    [syncServer, indexDocId, filePath],
  )
  const { ast, connected, error, connecting } = useSyncedAst(params)

  if (error) {
    return (
      <div style={{ padding: '16px' }}>
        <h2>Connection Error</h2>
        <p style={{ color: 'red' }}>{error}</p>
      </div>
    )
  }

  if (connecting) {
    return (
      <div style={{ padding: '16px' }}>
        <p>Connecting to sync server...</p>
      </div>
    )
  }

  if (!connected) {
    return (
      <div style={{ padding: '16px' }}>
        <p>Disconnected.</p>
      </div>
    )
  }

  if (!ast) {
    return (
      <div style={{ padding: '16px' }}>
        <p>Waiting for document...</p>
      </div>
    )
  }

  return <TodoFromAst ast={ast} filePath={filePath} />
}

function TodoFromAst({ ast, filePath }: { ast: RustQmdJson; filePath: string }) {
  const todoDiv = findTodoDiv(ast)

  if (!todoDiv) {
    return (
      <div style={{ padding: '16px' }}>
        <p style={{ color: '#888' }}>
          No <code>:::{'{'}#todo{'}'}</code> div found in <code>{filePath}</code>.
        </p>
        <details>
          <summary>Document has {ast.blocks.length} top-level blocks</summary>
          <pre style={{ fontSize: '12px', maxHeight: '200px', overflow: 'auto' }}>
            {ast.blocks.map(b => b.t).join(', ')}
          </pre>
        </details>
      </div>
    )
  }

  const items = extractTodoItems(todoDiv)

  return (
    <div style={{ padding: '16px' }}>
      <h2>Todo List</h2>
      <p style={{ fontSize: '12px', color: '#888' }}>
        Live from <code>{filePath}</code> — {items.length} item{items.length !== 1 ? 's' : ''}
      </p>
      <TodoList items={items} />
    </div>
  )
}
