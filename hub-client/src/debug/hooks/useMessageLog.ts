import { useState, useCallback } from 'react'
import type { MessageLogEntry } from '../types/messages'

const MAX_MESSAGES = 1000

export interface UseMessageLogResult {
  messages: MessageLogEntry[]
  addMessage: (entry: MessageLogEntry) => void
  clearMessages: () => void
}

/**
 * Maintain a rolling log of network messages, capped at MAX_MESSAGES to avoid
 * unbounded memory growth. Once the cap is reached, oldest entries are dropped.
 */
export function useMessageLog(): UseMessageLogResult {
  const [messages, setMessages] = useState<MessageLogEntry[]>([])

  const addMessage = useCallback((entry: MessageLogEntry) => {
    setMessages((prev) => {
      const next = [...prev, entry]
      if (next.length > MAX_MESSAGES) {
        return next.slice(-MAX_MESSAGES)
      }
      return next
    })
  }, [])

  const clearMessages = useCallback(() => {
    setMessages([])
  }, [])

  return { messages, addMessage, clearMessages }
}
