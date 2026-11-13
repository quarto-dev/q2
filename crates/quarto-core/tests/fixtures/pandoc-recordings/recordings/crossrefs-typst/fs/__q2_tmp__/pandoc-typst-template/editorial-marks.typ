// Editorial marks (Quarto 2; not part of the vendored Quarto 1 template).
//
// q2 renders `[++ ]`, `[-- ]`, `[!! ]` and `[>> ]` as calls to the functions below,
// and their `:::` block forms as calls to the `-block` variants. Every function takes
// the mark's `author` and `date` (strings, or `none`) and its content as `body`.
//
// To restyle the marks, redefine any of these functions with a `#let` of the same name
// in the document's header includes, which come after these definitions, so yours win:
//
//   header-includes: |
//     ```{=typst}
//     #let quarto-delete(author: none, date: none, body) = text(fill: gray)[(removed)]
//     ```

#let quarto-insert(author: none, date: none, body) = {
  underline(text(fill: rgb("#1a7f37"), body))
}

#let quarto-delete(author: none, date: none, body) = {
  strike(text(fill: rgb("#cf222e"), body))
}

#let quarto-highlight(author: none, date: none, body) = {
  highlight(fill: rgb("#fff3a3"), body)
}

#let quarto-edit-comment(author: none, date: none, body) = {
  box(
    fill: rgb("#ddeeff"),
    inset: (x: 3pt),
    outset: (y: 2pt),
    radius: 2pt,
    text(size: 0.85em, fill: rgb("#0b3d91"))[#if author != none [#strong(author): ]#body],
  )
}

#let quarto-insert-block(author: none, date: none, body) = {
  block(
    width: 100%,
    stroke: (left: 1.5pt + rgb("#1a7f37")),
    inset: (left: 8pt),
    underline(text(fill: rgb("#1a7f37"), body)),
  )
}

#let quarto-delete-block(author: none, date: none, body) = {
  block(
    width: 100%,
    stroke: (left: 1.5pt + rgb("#cf222e")),
    inset: (left: 8pt),
    strike(text(fill: rgb("#cf222e"), body)),
  )
}

#let quarto-highlight-block(author: none, date: none, body) = {
  block(width: 100%, highlight(fill: rgb("#fff3a3"), body))
}

#let quarto-edit-comment-block(author: none, date: none, body) = {
  block(
    width: 100%,
    fill: rgb("#ddeeff"),
    stroke: (left: 1.5pt + rgb("#0b3d91")),
    inset: 8pt,
    radius: 2pt,
    text(size: 0.9em, fill: rgb("#0b3d91"))[
      #if author != none [#strong(author)#linebreak()]
      #body
    ],
  )
}
