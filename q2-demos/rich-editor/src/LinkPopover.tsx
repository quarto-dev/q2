/**
 * The link popover, as in Google Docs (the design is the "Popover" option
 * of `mockups/inlines.html`), and the small form that makes or edits a link.
 *
 * The caret in a link, or the mouse resting over one, shows the link's
 * address under it, with Edit and Remove. The mouse shows it only once it
 * has rested on the link a moment (crossing a link on the way somewhere
 * else shows nothing), and it stays while the mouse is over the link or
 * the popover, and a moment longer to cross between them. Tab in the text
 * moves into the popover of the caret's link: its address (Enter opens the
 * link), then Edit and Remove; Escape, or Tab past either end, returns to
 * the text. Escape in the text puts the popover away until the caret
 * comes back to the link; the mouse resting on it shows it meanwhile, for
 * as long as it rests there.
 *
 * The form (from Edit, ⌘K or the toolbar's link button) has the link's
 * text and address: Enter or Apply makes it, Escape or Cancel goes back to
 * where it was opened from: the popover's Edit button, or the text. A
 * click elsewhere in the document closes it too, leaving the caret where
 * the click was. An empty address makes no link.
 *
 * Both are in the document's frame, laid over the page, outside the
 * editing root.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { createPortal } from 'react-dom'
import { escapeUri } from './editing/inlines.ts'
import { linkAround, linkAt, type LinkSpan } from './engine/commands.ts'
import { rectOf } from './engine/dom.ts'
import type { Position, Schema, Selection, State } from './engine/model.ts'

/** What the link form edits: the text from `from` to `to`, and the selection to go back to. */
export interface LinkForm {
  from: Position
  to: Position
  text: string
  url: string
  selection: Selection
  /** The link whose popover's Edit opened it (none: ⌘K or the toolbar). */
  fromEdit?: LinkSpan
}

interface LinkPopoverProps {
  doc: Document
  root: HTMLElement
  schema: Schema
  state: State
  writable: boolean
  form: LinkForm | null
  setForm: (form: LinkForm | null) => void
  setLink: (target: { from: Position; to: Position }, url: string | null, text?: string) => void
  focusAt: (selection: Selection) => void
}

/** How long the mouse rests on a link before its popover shows, and how long the popover outlasts it. */
const REST_MS = 250
const LINGER_MS = 300

/** Where the popover is until it's been measured: out of sight, but focusable. */
const UNPLACED = { left: -10000, top: 0 }

const keyOf = (link: LinkSpan | null) => (link ? `${link.from.field}:${link.from.offset}` : null)

export function LinkPopover({ doc, root, schema, state, writable, form, setForm, setLink, focusAt }: LinkPopoverProps) {
  const caretLink = linkAround(schema, state)
  // The link under the mouse, once it has rested there.
  const [hovered, setHovered] = useState<LinkSpan | null>(null)
  // The caret's link whose popover Escape put away, and the hovered one.
  const [dismissed, setDismissed] = useState<string | null>(null)
  const shunned = useRef<string | null>(null)
  // While the keyboard is in the popover: the link it's for, and the selection to go back to.
  // (`atEdit`: with its Edit button focused, not its first stop.)
  const [held, setHeld] = useState<{ link: LinkSpan; selection: Selection; atEdit?: boolean } | null>(null)
  const popover = useRef<HTMLDivElement>(null)

  // A dismissed popover comes back once the caret leaves its link.
  useEffect(() => {
    if (dismissed !== null && keyOf(caretLink) !== dismissed) setDismissed(null)
  }, [caretLink, dismissed])

  // A hovered link that's gone (edited away) shows nothing.
  const hoveredNow = hovered && linkAt(schema, state.fields, hovered.from)
  const link = form ? null : held ? held.link : (hoveredNow ?? (caretLink && keyOf(caretLink) !== dismissed ? caretLink : null))
  const forCaret = !held && link !== null && keyOf(link) === keyOf(caretLink)

  const latest = useRef({ state, link, forCaret, hovered })
  latest.current = { state, link, forCaret, hovered }

  // The mouse: resting on a link shows its popover; leaving both it and the popover hides it.
  useEffect(() => {
    let rest: { key: string; timer: number } | null = null
    let linger: number | null = null
    const view = doc.defaultView!
    const settle = (over: LinkSpan | null) => {
      if (rest) view.clearTimeout(rest.timer)
      rest = over && {
        key: keyOf(over)!,
        timer: view.setTimeout(() => {
          rest = null
          setHovered(over)
        }, REST_MS),
      }
    }
    const onMove = (e: MouseEvent) => {
      if (e.buttons) return
      const target = e.target as Element | null
      const a = target?.closest?.('a[data-qe-at]')
      const field = a?.closest<HTMLElement>('[data-field]')
      const over =
        a && field && root.contains(a)
          ? linkAt(schema, latest.current.state.fields, { field: field.dataset.field!, offset: Number(a.getAttribute('data-qe-at')) })
          : null
      if (over || target?.closest?.('.qe-linkbar')) {
        if (linger !== null) view.clearTimeout(linger)
        linger = null
        if (!over) return
        if (keyOf(over) !== shunned.current) shunned.current = null
        if (keyOf(over) === keyOf(latest.current.hovered) || shunned.current) return settle(null)
        if (rest?.key !== keyOf(over)) settle(over)
        return
      }
      settle(null)
      if (shunned.current) shunned.current = null
      else if (latest.current.hovered && linger === null) {
        linger = view.setTimeout(() => {
          linger = null
          setHovered(null)
        }, LINGER_MS)
      }
    }
    doc.addEventListener('mousemove', onMove)
    return () => {
      doc.removeEventListener('mousemove', onMove)
      if (rest) view.clearTimeout(rest.timer)
      if (linger !== null) view.clearTimeout(linger)
    }
  }, [doc, root, schema])

  // The keyboard in the text: Escape puts the popover away, Tab goes into it.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (doc.activeElement !== root) return
      const { link, forCaret, state, hovered } = latest.current
      if (e.key === 'Escape' && link) {
        e.preventDefault()
        if (forCaret) setDismissed(keyOf(link))
        shunned.current = keyOf(hovered)
        setHovered(null)
      } else if (e.key === 'Tab' && !e.shiftKey && !e.metaKey && !e.ctrlKey && !e.altKey && forCaret && link && state.selection) {
        e.preventDefault()
        setHeld({ link, selection: state.selection })
      }
    }
    doc.addEventListener('keydown', onKey, true)
    return () => doc.removeEventListener('keydown', onKey, true)
  }, [doc, root])

  // A press in the document outside the form closes it, as Cancel does,
  // but the caret goes where the press was.
  useEffect(() => {
    if (!form) return
    const onDown = (e: MouseEvent) => {
      if (!box.current?.contains(e.target as Node)) setForm(null)
    }
    doc.addEventListener('mousedown', onDown, true)
    return () => doc.removeEventListener('mousedown', onDown, true)
  }, [form, doc, setForm])

  // Into the popover once it's held for the keyboard.
  useEffect(() => {
    if (!held || popover.current?.contains(doc.activeElement)) return
    const stops = [...(popover.current?.querySelectorAll<HTMLElement>('a[href], button') ?? [])]
    ;(held.atEdit ? stops.find((el) => el.textContent === 'Edit') : stops[0])?.focus()
  }, [held, doc])

  const backToText = useCallback(() => {
    const selection = held?.selection
    setHeld(null)
    if (selection) focusAt(selection)
  }, [held, focusAt])

  const onPopoverKey = (e: ReactKeyboardEvent) => {
    if (e.key !== 'Escape' && e.key !== 'Tab') return
    e.preventDefault()
    e.stopPropagation()
    const stops = [...(popover.current?.querySelectorAll<HTMLElement>('a[href], button') ?? [])]
    const at = stops.indexOf(doc.activeElement as HTMLElement)
    // Moved here, not by the browser: Safari's Tab skips buttons and links.
    const next = e.key === 'Tab' ? stops[at + (e.shiftKey ? -1 : 1)] : undefined
    if (next) next.focus()
    else backToText()
  }

  const edit = (target: LinkSpan) => {
    const text = rootText(root, target) ?? ''
    const selection = held?.selection ?? state.selection ?? { anchor: target.to, focus: target.to }
    setHeld(null)
    setHovered(null)
    setForm({ from: target.from, to: target.to, text, url: target.url, selection, fromEdit: target })
  }
  const remove = (target: LinkSpan) => {
    // From the keyboard, the text has no selection while it's in here: put
    // it back first, so undo returns to it.
    const selection = held?.selection
    setHeld(null)
    setHovered(null)
    if (selection) focusAt(selection)
    setLink(target, null)
  }

  // Where the popover (or the form) goes: under the link (or the text the form is for).
  const [place, setPlace] = useState<{ left: number; top: number } | null>(null)
  const box = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    const target = form ?? link
    const rect = form
      ? rectOf(root, form.from, form.to)
      : link
        ? ([...(root.querySelector(`a[data-qe-at="${link.from.offset}"]`)?.getClientRects() ?? [])].pop() ?? null)
        : null
    if (!target || !rect) {
      if (place) setPlace(null)
      return
    }
    const view = doc.defaultView!
    const width = box.current?.offsetWidth ?? 0
    const left = Math.max(8, Math.min(rect.left + view.scrollX, doc.documentElement.clientWidth - width - 8))
    const top = rect.bottom + view.scrollY + 6
    if (place?.left !== left || place?.top !== top) setPlace({ left, top })
  })

  if (!writable) return null
  if (form) {
    return createPortal(
      <div ref={box} className="qe-bubble qe-linkform" style={place ?? UNPLACED}>
        <Form
          form={form}
          onCancel={() => {
            setForm(null)
            if (form.fromEdit) setHeld({ link: form.fromEdit, selection: form.selection, atEdit: true })
            else focusAt(form.selection)
          }}
          onApply={(text, typed) => {
            setForm(null)
            const url = escapeUri(typed.trim())
            const label = text || typed.trim()
            if (!label) focusAt(form.selection)
            else setLink(form, url || null, label)
          }}
        />
      </div>,
      doc.body,
    )
  }
  if (!link) return null
  return createPortal(
    <div
      ref={(el) => {
        popover.current = el
        box.current = el
      }}
      className="qe-bubble qe-linkbar"
      role="dialog"
      aria-label="Link"
      style={place ?? UNPLACED}
      onKeyDown={onPopoverKey}
      onBlur={(e) => {
        if (held && !popover.current?.contains(e.relatedTarget as Node | null) && e.relatedTarget !== root) setHeld(null)
      }}
      onMouseDown={(e) => {
        if ((e.target as Element).closest('button')) e.preventDefault()
      }}
    >
      {link.url ? (
        <a
          className="qe-url"
          href={link.url}
          target="_blank"
          rel="noopener noreferrer"
          // Once it's opened, the keyboard is back in the text.
          onClick={() => doc.defaultView?.setTimeout(backToText)}
        >
          {link.url}
        </a>
      ) : (
        <span className="qe-none">No address</span>
      )}
      <span className="qe-bsep" />
      <button type="button" onClick={() => edit(link)}>
        Edit
      </button>
      <button type="button" onClick={() => remove(link)}>
        Remove
      </button>
    </div>,
    doc.body,
  )
}

/** The link form's fields and buttons. */
function Form({ form, onCancel, onApply }: { form: LinkForm; onCancel: () => void; onApply: (text: string, url: string) => void }) {
  const [text, setText] = useState(form.text)
  const [url, setUrl] = useState(form.url)
  const first = useRef<HTMLInputElement>(null)
  const second = useRef<HTMLInputElement>(null)
  useEffect(() => {
    ;(form.text ? second : first).current?.focus()
  }, [form])
  return (
    <form
      role="dialog"
      aria-label="Link"
      onSubmit={(e) => {
        e.preventDefault()
        onApply(text, url)
      }}
      onKeyDown={(e) => {
        e.stopPropagation()
        if (e.key === 'Escape') {
          e.preventDefault()
          onCancel()
        }
      }}
    >
      <label>
        Text
        <input ref={first} name="text" value={text} onChange={(e) => setText(e.target.value)} />
      </label>
      <label>
        Link
        <input ref={second} name="url" value={url} placeholder="Paste or type an address" onChange={(e) => setUrl(e.target.value)} />
      </label>
      <div className="qe-row">
        <button type="button" onClick={onCancel}>
          Cancel
        </button>
        <button type="submit" className="qe-apply">
          Apply
        </button>
      </div>
    </form>
  )
}

/** The visible text from `link.from` to `link.to`, as the page shows it. */
function rootText(root: HTMLElement, link: LinkSpan): string | null {
  const a = root.querySelector(`a[data-qe-at="${link.from.offset}"]`)
  return a ? (a.textContent ?? '').replaceAll('​', '') : null
}
