/**
 * The toolbar's tooltips (the design is the "Chosen" section of
 * `mockups/shortcut-tooltips.html`: light, like the style menu). Resting
 * the mouse on a button shows its name and keyboard shortcut under it: the
 * button's `data-tip` and `data-keys`. The first waits a moment; while the
 * mouse goes on along the toolbar, the next show at once, as a Mac's do.
 * Pressing a button puts its tooltip away until the mouse leaves the
 * button.
 *
 * A disabled button shows its tooltip too. It takes no mouse events (the
 * toolbar's CSS lets them through to the toolbar), so the toolbar finds
 * the button under the mouse itself.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState, type MouseEvent, type ReactNode, type RefObject } from 'react'

/** How long the mouse rests on a button before its tooltip shows, in ms. */
const DELAY = 500

/** For how long after a tooltip goes the next one shows at once, in ms. */
const WARM = 400

/** How close a tooltip comes to the toolbar's ends, in px. */
const MARGIN = 4

interface Tip {
  label: string
  keys: string | null
  /** Where its top centre goes, from the toolbar's top left. */
  x: number
  y: number
}

export interface Tooltips {
  onMouseMove: (e: MouseEvent) => void
  onMouseLeave: () => void
  onMouseDown: () => void
  /** The tooltip, to put in the toolbar (which positions it). */
  element: ReactNode
}

export function useTooltips(bar: RefObject<HTMLElement | null>): Tooltips {
  const [tip, setTip] = useState<Tip | null>(null)
  // The button under the mouse (its tooltip may have been put away).
  const target = useRef<HTMLElement | null>(null)
  const timer = useRef(0)
  const shown = useRef(false)
  const warmUntil = useRef(0)
  const element = useRef<HTMLDivElement>(null)

  useEffect(() => () => clearTimeout(timer.current), [])

  // Centred under its button, but within the toolbar: the first button's
  // would stick out of its left end.
  useLayoutEffect(() => {
    const container = bar.current
    if (!tip || !element.current || !container) return
    const half = element.current.offsetWidth / 2
    const x = Math.min(Math.max(tip.x, half + MARGIN), container.clientWidth - half - MARGIN)
    if (x !== tip.x) setTip({ ...tip, x })
  }, [tip, bar])

  const hide = useCallback(() => {
    clearTimeout(timer.current)
    if (shown.current) warmUntil.current = Date.now() + WARM
    shown.current = false
    setTip(null)
  }, [])

  const onMouseMove = useCallback(
    (e: MouseEvent) => {
      const container = bar.current
      if (!container) return
      const button =
        [...container.querySelectorAll<HTMLElement>('[data-tip]')].find((b) => {
          const r = b.getBoundingClientRect()
          return e.clientX >= r.left && e.clientX < r.right && e.clientY >= r.top && e.clientY < r.bottom
        }) ?? null
      if (button === target.current) return
      target.current = button
      hide()
      if (!button) return
      const show = () => {
        const b = container.getBoundingClientRect()
        const r = button.getBoundingClientRect()
        shown.current = true
        setTip({
          label: button.dataset.tip ?? '',
          keys: button.dataset.keys ?? null,
          x: r.left - b.left + r.width / 2,
          y: r.bottom - b.top + 6,
        })
      }
      timer.current = window.setTimeout(show, Date.now() < warmUntil.current ? 0 : DELAY)
    },
    [bar, hide],
  )

  const onMouseLeave = useCallback(() => {
    target.current = null
    hide()
  }, [hide])

  const tooltip = tip && (
    <div ref={element} className="qe-tip" role="tooltip" style={{ left: tip.x, top: tip.y }}>
      <span>{tip.label}</span>
      {tip.keys && <kbd>{tip.keys}</kbd>}
    </div>
  )

  return { onMouseMove, onMouseLeave, onMouseDown: hide, element: tooltip }
}
