/** Window rectangles for the demo (DM5). All values are screen points. */

export interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface Layout {
  hostEditor: Rect;
  hostTerminal: Rect;
  guestEditor: Rect;
  guestTerminal?: Rect;
}

/** The visible frame of the main monitor: the menu bar takes `menuBar` points off the top. */
export function mainMonitorVisibleFrame(s: { width: number; height: number; menuBar: number }): Rect {
  return { left: 0, top: s.menuBar, width: s.width, height: s.height - s.menuBar };
}

/** Two equal columns; in each, editor and terminal split 3:1 (the editor rounds up). */
export function computeLayout(frame: Rect, opts: { guestTerminal: boolean }): Layout {
  const colW = Math.floor(frame.width / 2);
  const editorH = Math.round(frame.height * 0.75);
  const termH = frame.height - editorH;
  const hostEditor = { left: frame.left, top: frame.top, width: colW, height: editorH };
  const hostTerminal = { left: frame.left, top: frame.top + editorH, width: colW, height: termH };
  const rightLeft = frame.left + colW;
  if (!opts.guestTerminal) {
    return { hostEditor, hostTerminal, guestEditor: { left: rightLeft, top: frame.top, width: colW, height: frame.height } };
  }
  return {
    hostEditor,
    hostTerminal,
    guestEditor: { left: rightLeft, top: frame.top, width: colW, height: editorH },
    guestTerminal: { left: rightLeft, top: frame.top + editorH, width: colW, height: termH },
  };
}
