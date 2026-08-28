# Hub-client design system

The hub-client UI is built from a small set of tokens and primitives.
This page is the contract: follow it when adding or touching UI, and the
app stays coherent. The enforcement counterparts are `npm run lint:css`
(off-token values) and the `#/dev/` gallery (keyboard + axe coverage).

## Tokens (`src/theme.css`)

Three layers — new values land on the right one:

1. **Primitive ramps** — raw palette/scale values (`--posit-teal`,
   `--posit-blue-dark-2`). Never referenced directly by components.
2. **Semantic aliases** — meaning-named tokens (`--text-primary`,
   `--border-color`, `--editor-accent-bg`), defined per theme.
3. **Scale tokens** — the shared scales: spacing (`--space-1`…`--space-8`,
   4px base), radii (`--radius-sm/md/lg`), elevation (`--shadow-1/2/3`),
   z-layers (`--z-sticky`…`--z-max`), type (`--text-xs`…`--text-xl`,
   `--font-weight-*`, `--leading-*`, `--font-mono`), motion
   (`--duration-fast/base`, `--ease-out/standard`), and `--focus-ring`.

Rules: no hex/rgb colors outside `theme.css`; no bare z-index integers;
no `outline: none` without a `:focus-visible` counterpart; logical
properties (`margin-inline-start`, `inset-inline-end`) over physical ones.
All enforced by `lint:css`.

## Primitives

| Primitive | Where | Notes |
|---|---|---|
| Buttons | `ui.css` `.qh-btn` (+ `.primary` `.outline` `.danger` `.ghost-accent`, `.small`), `.qh-icon-btn` (+ `.boxed`), `.qh-link` | Two sizes; disabled + focus-visible live on the base classes. Not buttons: `.view-toggle-btn` (segmented control), `.qh-pager` (nav strip), `.preview-btn` (header primary pill). |
| Menu | `components/Menu.tsx` | The only action menu. APG menu-button pattern: arrows/Home/End, type-ahead, submenus, Escape + focus return. Destructive items use `danger` and must be confirm-guarded or undoable. Popovers containing forms (avatar menu) are not menus — they use `.qh-menu` styling only. |
| Tooltip | `components/Tooltip.tsx` | The only tooltip. Never use `title=`. Non-interactive content only. |
| Notifications | `components/notifications.css` | Three tiers — transient (auto-dismiss), dismissible persistent, session banner. Pick by how long the information must live; see the file header. |
| Dialogs | `components/ModalDialog.tsx` | Every dialog routes through it (focus trap, restoration, Escape). Structure content with `.dialog-content` / `.dialog-actions`; form dialogs add `.qh-form-dialog`. |
| Form controls | `ui.css` `.qh-input`, `.qh-field-label`, `.qh-tabs` | Validation: `aria-invalid` + error text via `aria-describedby` (field-level) or `.qh-error.inline` (form-level). |
| Icons | `components/icons.tsx` | The only icon source. Decorative (`aria-hidden`), 24×24 stroke style, `currentColor`. |
| Utilities | `ui.css` `.qh-truncate`, `.qh-row-hover`, `.qh-active-accent-row` | Single-purpose shared classes — adopt, don't fork. |

## The gallery (`#/dev/gallery`)

Every primitive renders on the gallery page in its meaningful states, in
both themes, covered by axe-core scans and behavioral specs
(`npm run test:harness`). The tokens gallery is `#/dev/tokens`.

## Adding a component

1. Style with tokens only — if a value has no token, add one at the right
   layer in `theme.css` (or use the nearest scale step).
2. Reuse the primitives above. If none fits, that's a design decision —
   build it accessibly (keyboard path, ARIA pattern from the APG) and add
   it here.
3. Add the component to the gallery in its default/hover/focus/disabled/
   error states; the axe baseline picks it up from there.
4. Every pointer affordance needs a keyboard path; every icon-only control
   needs an `aria-label`; status is never conveyed by color alone.
