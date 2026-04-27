import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './debug.css'
import { DebugApp } from './DebugApp'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <DebugApp />
  </StrictMode>,
)
