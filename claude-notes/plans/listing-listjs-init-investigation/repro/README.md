# bd-nbv80e33 repro

Three posts, `page-size: 2`, categories + sort-ui + filter-ui on a default
listing. Render with `q2 render` (or `cargo run --bin q2 -- render` from
here) and inspect `_site/index.html`: Q1 would emit an inline
`new List('listing-posts', ...)` init script, a pagination `<nav>`, a
sort `<select>` and a filter `<input class="search">`. Compare against Q1
with `quarto render --output-dir _site-q1`.
