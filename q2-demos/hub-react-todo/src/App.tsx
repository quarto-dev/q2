/**
 * Root app component — hardcoded to a specific sync server + document for demo purposes.
 */

import { TodoApp } from './TodoApp.tsx'

const syncServer = 'wss://sync.automerge.org'
const indexDocId = 'FqXQmLvicAYfARgVMdSjrsMiS54'
const filePath = 'todo.qmd'

export function App() {
  return (
    <div style={{ fontFamily: 'system-ui, sans-serif', maxWidth: '600px', margin: '0 auto', padding: '20px' }}>
      <h1 style={{ fontSize: '24px' }}>Quarto Hub - React Todo</h1>

      <div style={{ marginBottom: '12px', padding: '8px', background: '#f0f0f0', borderRadius: '4px', fontSize: '13px' }}>
        <strong>Server:</strong> {syncServer}
        <br />
        <strong>Document:</strong> <code>{indexDocId}</code>
        <br />
        <strong>File:</strong> <code>{filePath}</code>
      </div>

      <TodoApp
        syncServer={syncServer}
        indexDocId={indexDocId}
        filePath={filePath}
      />
    </div>
  )
}
