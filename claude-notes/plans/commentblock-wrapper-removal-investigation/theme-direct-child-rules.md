# Theme direct-child selectors that the CommentBlock wrapper defeats

Generated 2026-09-10 from `resources/scss` (grep of `<parent> > <child>` selectors whose
child can be a commentable block). Counts are rule occurrences across all SCSS files.

```
   3 .callout-body > div
   2 section > section
   2 li > ul
   2 li > span
   2 li > a.active
   2 li > a:hover
   2 li > a
   2 .callout-body > :first-child
   1 th > p:last-of-type
   1 th > li
   1 li > p:last-of-type
   1 li > #
   1 figure > figcaption
   1 .tab-pane > pre:last-child
   1 .tab-pane > p:nth-child(1)
   1 .tab-pane > p:last-child
   1 .tab-pane > p
   1 .content > section:first-of-type
   1 .content > p:has(
   1 .columns > .column:last-child
   1 .columns > .column:first-child
   1 .columns > .column
   1 .callout-body > :last-child:not(.sourceCode)
   1 .callout-body > :last-child
   1 .callout > div.callout-header
   1 .callout > .callout-header.collapsed
   1 .callout > .callout-header
   1 .callout > .callout-body
```

Chrome-eligible block components (the only hosts `CommentBlock` ever wraps — see `canHoldComment`
in `custom/CommentBlock.tsx`): `Para`, `Plain`, `Header`, `CodeBlock` (and the `MermaidCodeBlock`
override), and the `Div.quarto-edit-comment-container`. Everything else (lists as a whole, tables,
figures, custom nodes such as Callout/Theorem) already renders passthrough with no wrapper.
