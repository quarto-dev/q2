# Repro output at HEAD 15bb0d54f (2026-10-09)

Run `q2 render` in `repro/`.

All 5 pages:

    Warning [Q-12-10]: Re-parsing rendered listing `tbl` failed with 1 diagnostic(s); first: Unclosed Underscore Emphasis. Listing skipped.
    Warning [Q-12-10]: Re-parsing rendered listing `dflt` failed with 1 diagnostic(s); first: Unclosed Underscore Emphasis. Listing skipped.

Without p/a.qmd and p/c.qmd (the `_scope` titles):

    Warning [Q-12-10]: Re-parsing rendered listing `tbl` produced 1 diagnostic(s); first: HTML element converted to raw HTML
    Warning [Q-12-10]: Re-parsing rendered listing `dflt` produced 1 diagnostic(s); first: HTML element converted to raw HTML

    <a href="p/d.html" class="no-anchor no-external listing-title">About <anonymous> frames</a></h3>
    <a href="p/b.html" class="no-anchor no-external listing-title">Plan for q2 preview and emph</a></h3>
    <td><a href="p/d.html" class="no-external">About <anonymous> frames</a>
    <td><a href="p/b.html" class="no-external">Plan for q2 preview and emph</a>

Each page renders its own title correctly (`<code>_scope</code>`, `<code>&lt;anonymous&gt;</code>`).
