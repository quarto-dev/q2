# Screen-reader & high-contrast smoke script

Manual accessibility pass for the hub-client. Automated coverage lives in
`e2e/` (axe baselines, keyboard walkthroughs, forced-colors emulation);
this script is the human counterpart — run it before shipping changes to
the header, sidebar, dialogs, menus, or notifications.

## VoiceOver pass (macOS)

Setup: `npm run dev`, open http://localhost:5173, open or create a
project so the editor shell is up. Toggle VoiceOver with ⌘F5. Navigate
with VO+←/→ (VO = Ctrl+Option) and interact with VO+Space; use the rotor
(VO+U) for landmarks and headings.

### Header

1. Tab from the top of the page. The skip link ("Skip to main content")
   appears first and moves focus to the editor.
2. Every icon-only button announces its `aria-label` (e.g. "Switch
   project", "Share") — never just "button".
3. The connection indicator announces "Online"/"Offline" as text, not
   color alone.

### File tree (sidebar, FILES section)

1. The section header announces "FILES, expanded/collapsed, button" and
   VO+Space toggles it.
2. The tree announces as a tree; rows announce name, level, and
   selected/collapsed state.
3. Arrow keys move between rows; →/← expand and collapse folders;
   typing a letter jumps by name; Enter opens the focused file.
4. Shift+F10 on a row opens the actions menu; Escape closes it and
   focus returns to the row.

### Outline (sidebar, OUTLINE section)

1. Rows activate with VO+Space/Enter and move the editor cursor.
2. Collapse toggles announce "Collapse/Expand <name>, collapsed/expanded".

### Dialogs

1. Opening a dialog (e.g. Share) moves focus inside it; the dialog
   announces its title and "modal".
2. Tab cycles within the dialog and never escapes to the page behind.
3. Escape closes; focus returns to the control that opened it.

### Notifications

1. A toast (e.g. the auto-save confirmation on ⌘S) is announced via its
   `role="status"` live region without stealing focus.
2. The update-available toast's Reload and Dismiss buttons are reachable
   and labelled; the ephemeral-session banner explains itself on focus.

## Windows High Contrast (forced colors) pass

Automated coverage: `e2e/forced-colors.harness.spec.ts` emulates
`forced-colors: active` and asserts buttons, menus, dialogs, tooltips,
and the selected file row keep visible boundaries.

Manual pass (Windows, Edge/Chrome): Settings → Accessibility → Contrast
themes → choose a theme. Then verify:

1. All buttons, inputs, menus, and dialogs have visible borders.
2. The selected file in the tree is distinguishable from other rows.
3. Focus indicators are visible on every control you can Tab to.
4. Diagnostic severity markers in the editor remain distinguishable
   (they keep their colors intentionally — color is the semantic there).
