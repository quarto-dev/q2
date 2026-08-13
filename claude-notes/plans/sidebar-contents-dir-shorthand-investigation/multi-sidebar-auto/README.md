# Probe: multi-sidebar `auto:` selection

Investigation artifact for `bd-sidebar-contents-dir-shorthand-z7arvhx8`.
Plan: `claude-notes/plans/2026-08-12-sidebar-contents-dir-shorthand.md`.

## What this probes

Whether q2 can select a sidebar whose membership is defined only by an
**unexpanded `auto:` entry**, in a project with **more than one** sidebar.

This uses only syntax q2 supports today (`auto: how-to`), so it is
independent of the `contents: <dir>` shorthand bug itself. It isolates the
*second* defect found during investigation:

- `sidebar_for_page` runs at `transforms/sidebar_generate.rs:89`
- `expand_auto` runs at `transforms/sidebar_generate.rs:127`

so selection sees unexpanded contents, and `contains_source_path`
(`quarto-navigation/src/sidebar.rs:647`) ignores `SidebarEntry::Auto(_)`
(`:663`).

Two sidebars are declared, both with `id:`, which defeats the
single-sidebar wildcard (`sidebar.rs:637`) and forces Rule 3 containment
matching — the same configuration shape as the Connect docs.

## Confirmed failure at `main` @ `152ed8fb` (2026-08-12)

```bash
cargo run --bin q2 -- render claude-notes/plans/sidebar-contents-dir-shorthand-investigation/multi-sidebar-auto
cd _site && for f in how-to/index.html how-to/one.html how-to/two.html other/alpha.html index.html; do
  printf "%-22s sidebar=%s\n" "$f" "$(grep -c 'id="quarto-sidebar"' $f)"
done
```

Observed:

```
how-to/index.html      sidebar=0
how-to/one.html        sidebar=0
how-to/two.html        sidebar=0
other/alpha.html       sidebar=1
index.html             sidebar=0
```

All three `how-to` pages carry **no** `#quarto-sidebar` element, exactly as
the ordering analysis predicts: the `howto` sidebar's only entry is an
unexpanded `Auto`, and containment cannot match through it.
`other/alpha.html` still gets its sidebar because it names a page directly.
(`index.html` having none is expected — it is in neither sidebar.)

This confirms the defect is **pre-existing and independent of the
`contents: <dir>` shorthand**, and that fixing only the shorthand's parse
would leave the Connect-docs failure in place.

## Expected after a fix

All three `how-to` pages get the `howto` sidebar, with entries for
`index`, `one` and `two`.

## Expected after a fix

All three `how-to` pages get the `howto` sidebar, with entries for
`index`, `one` and `two`.
