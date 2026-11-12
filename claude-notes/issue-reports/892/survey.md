# Task-list ("checklist") rendering: an ecosystem survey

Compiled 2026-11-12 for the Quarto 2 task-list layout bug (bullet + checkbox both visible, wonky layout).

## Method and evidence levels

Wherever possible the renderer was **actually run** on the same fixtures; otherwise the renderer's source or test fixtures were read. Each section is tagged:

- **\[RUN\]**: output below is verbatim from running the named version locally (or the live GitHub API).
- **\[SRC\]**: derived from reading the renderer's source or its own spec/test fixtures (URL given). It was not executed.
- **\[UNVERIFIED\]**: secondary sources only, or an inference. Treat it as a lead, not a fact.

Fixtures used (scratchpad `t/`):

```markdown
<!-- tight.md -->
- [ ] one
- [x] two
- plain
```

```markdown
<!-- edge.md -->
1. [ ] ord one
2. [x] ord two

- [ ] loose one

- [x] loose two

  second para

- [ ] parent
  - [x] child
  - [ ] child2

- [ ]
- [X] upper
- [ ]nospace
- plain

  [ ] task marker in second para
```

(The second bullet list in `edge.md` is *loose*, so its items get `<p>`.)

---

## 1. GFM spec, cmark-gfm, github.com

### 1a. GFM spec (0.29-gfm, 2019-04-06) \[SRC\]

Source: `test/spec.txt` in github/cmark-gfm, § "Task list items (extension)" (also at https://github.github.com/gfm/#task-list-items-extension-).

> A task list item is a list item where the first block in it is a paragraph which begins with a task list item marker and at least one whitespace character before any other content.
>
> A task list item marker consists of an optional number of spaces, a left bracket (`[`), either a whitespace character or the letter `x` in either lowercase or uppercase, and then a right bracket (`]`).
>
> This spec does not define how the checkbox elements are interacted with: in practice, implementors are free to render the checkboxes as disabled or inmutable elements, or they may dynamically handle dynamic interactions ...

Spec examples (note: no classes, no label, `disabled=""`, one space before text):

```html
<ul>
<li><input disabled="" type="checkbox"> foo</li>
<li><input checked="" disabled="" type="checkbox"> bar</li>
</ul>
```

The spec says nothing about classes, CSS, ordered lists, loose lists, or mixed lists. Only bullet lists appear in its examples, but the definition says "list item", so ordered lists qualify.

### 1b. cmark-gfm (reference C impl) \[RUN + SRC\]

Run via Python `cmarkgfm` 2025.10.22, which bundles cmark-gfm `0.29.0.gfm.13`. Source: `extensions/tasklist.c` (github/cmark-gfm master):

```c
cmark_strbuf_puts(renderer->html, "<li");
...
if (node->as.list.checked) {
  cmark_strbuf_puts(renderer->html, "<input type=\"checkbox\" checked=\"\" disabled=\"\" /> ");
} else {
  cmark_strbuf_puts(renderer->html, "<input type=\"checkbox\" disabled=\"\" /> ");
}
```

Tight:

```html
<ul>
<li><input type="checkbox" disabled="" /> one</li>
<li><input type="checkbox" checked="" disabled="" /> two</li>
<li>plain</li>
</ul>
```

Loose. The checkbox is emitted by the `<li>` renderer itself, so it lands **before** the `<p>`, not inside it:

```html
<li><input type="checkbox" checked="" disabled="" /> 
<p>loose two</p>
<p>second para</p>
</li>
```

- No classes anywhere, and no CSS.
- Ordered lists work: `<ol><li><input ...> ord one</li>`.
- Empty `- [ ]`, `[ ]nospace`, and a marker in a second paragraph are **not** tasks. `[X]` is a task.

### 1c. github.com (live) \[RUN\]

Run via `gh api /markdown` with `mode: gfm` on 2026-11-12. This is GitHub's production pipeline, which post-processes cmark-gfm and differs from it.

Tight:

```html
<ul class="contains-task-list">
<li class="task-list-item"><input type="checkbox" id="" disabled="" class="task-list-item-checkbox" aria-label="Incomplete task"> one</li>
<li class="task-list-item"><input type="checkbox" id="" disabled="" class="task-list-item-checkbox" aria-label="Completed task" checked=""> two</li>
<li>plain</li>
</ul>
```

Edge (abridged):

```html
<ol class="contains-task-list">
<li class="task-list-item"><input type="checkbox" id="" disabled="" class="task-list-item-checkbox" aria-label="Incomplete task"> ord one</li>
...
<ul class="contains-task-list">
<li class="task-list-item">
<p><input type="checkbox" id="" disabled="" class="task-list-item-checkbox" aria-label="Incomplete task"> loose one</p>
</li>
...
<li class="task-list-item">
<p><input ... aria-label="Incomplete task"> parent</p>
<ul class="contains-task-list">
<li class="task-list-item"><input ... checked=""> child</li>
...
<li>
<p>[ ]</p>                       <!-- empty marker: NOT a task -->
</li>
<li class="task-list-item">
<p><input ... aria-label="Completed task" checked=""> upper</p>
</li>
<li>
<p>[ ]nospace</p>                <!-- NOT a task -->
</li>
<li>
<p>plain</p>
<p>[ ] task marker in second para</p>   <!-- NOT a task -->
</li>
```

- Classes: `ul|ol.contains-task-list`, `li.task-list-item` (task items only), `input.task-list-item-checkbox`.
- A mixed list **still gets** `contains-task-list`. The plain `<li>` has no class.
- Loose list: the checkbox goes **inside** the `<p>` (unlike cmark-gfm).
- There is no `<label>`. a11y comes from `aria-label="Incomplete task"` / `"Completed task"`, which does **not** include the item text.
- The checkbox is `disabled`. GitHub makes it interactive with JS on issues/PRs, where the `.enabled` and `.handle` classes appear.

GitHub CSS: `github-markdown-css` (sindresorhus) mirrors GitHub's own styles. Commit `7f38dcb905`, 2026-09-18, `github-markdown.css`:

```css
.markdown-body ul,
.markdown-body ol { margin-top: 0; margin-bottom: 0; padding-left: 2em; }

.markdown-body .task-list-item { list-style-type: none; }
.markdown-body .task-list-item label { font-weight: var(--base-text-weight-normal, 400); }
.markdown-body .task-list-item.enabled label { cursor: pointer; }
.markdown-body .task-list-item+.task-list-item { margin-top: var(--base-size-4); }
.markdown-body .task-list-item .handle { display: none; }
.markdown-body .task-list-item-checkbox {
  margin: 0 .2em .25em -1.4em;
  vertical-align: middle;
}
.markdown-body ul:dir(rtl) .task-list-item-checkbox { margin: 0 -1.6em .25em .2em; }
.markdown-body ol:dir(rtl) .task-list-item-checkbox { margin: 0 -1.6em .25em .2em; }
```

- The bullet is suppressed **per item** (`li.task-list-item`), not per list. A mixed list therefore keeps bullets on its plain items and drops them on task items.
- List padding is unchanged (2em at every nesting level). The checkbox is pulled into the marker gutter with a negative left margin (`-1.4em`), so the text stays aligned with ordinary list text.
- Because the selector is on `li`, **ordered task items lose their numbers too**.

Historical note \[SRC\]: GitHub's older open-source filter, `github-archive/task_list` (`lib/task_list/filter.rb`), used `ul.task-list` / `li.task-list-item` / `input.task-list-item-checkbox` with `disabled="disabled"`. It put the checkbox inside `p[1]` when one exists. GitLab inherited the `task-list` name from it (see §6).

---

## 2. Pandoc

Run with pandoc **3.13** \[RUN\]. Source: `src/Text/Pandoc/Writers/HTML.hs` (jgm/pandoc main) \[SRC\].

### AST

`task_lists` is a **reader** extension. It is on in pandoc `markdown` and `gfm` and off in `commonmark` unless enabled. The reader rewrites the marker into a ballot-box character at the head of the first `Plain`/`Para`:

```
[ BulletList
    [ [ Plain [ Str "\9744" , Space , Str "one" ] ]     -- ☐ U+2610
    , [ Plain [ Str "\9746" , Space , Str "two" ] ]     -- ☒ U+2612
    , [ Plain [ Str "plain" ] ]
    ]
]
```

An empty item is `Plain [ Str "\9744" ]`. The HTML writer detects tasks from the AST through `toTaskListItem`, so source text that literally starts with `☐ ` becomes a checkbox even under `markdown-task_lists`. That was observed: `- ☐ a` produced `<ul class="task-list"><li><label><input type="checkbox" />a</label></li>`.

### HTML writer

```haskell
-- list items
case toTaskListItem bls of
  Just (checked, (Para is:bs))  -> taskListItem checked H.p is bs
  Just (checked, (Plain is:bs)) -> taskListItem checked id is bs
  ...
  taskListItem checked constr is bs = do
    let checkbox  = if checked then checkbox' ! A.checked "" else checkbox'
        checkbox' = H.input ! A.type_ "checkbox"
    ...
    return $ constr (H.label (checkbox >> isContents)) >> ...

-- BulletList only:
(if isJust (mapM toTaskListItem lst) then (! A.class_ "task-list") else id) <$> unordList opts contents
-- OrderedList: no task-list class branch at all
```

Tight `-f markdown` (mixed list):

```html
<ul>
<li><label><input type="checkbox" />one</label></li>
<li><label><input type="checkbox" checked="" />two</label></li>
<li>plain</li>
</ul>
```

All-task list:

```html
<ul class="task-list">
<li><label><input type="checkbox" />a</label></li>
<li><label><input type="checkbox" checked="" />b</label></li>
</ul>
```

Edge `-f markdown`:

```html
<ol type="1">                                   <!-- ordered: tasks work, but NO class -->
<li><label><input type="checkbox" />ord one</label></li>
<li><label><input type="checkbox" checked="" />ord two</label></li>
</ol>
<ul>                                            <!-- mixed: NO class -->
<li><p><label><input type="checkbox" />loose one</label></p></li>
<li><p><label><input type="checkbox" checked="" />loose two</label></p>
<p>second para</p></li>
<li><p><label><input type="checkbox" />parent</label></p>
<ul class="task-list">                          <!-- nested all-task list: class -->
<li><label><input type="checkbox" checked="" />child</label></li>
<li><label><input type="checkbox" />child2</label></li>
</ul></li>
<li><p><label><input type="checkbox" /></label></p></li>   <!-- empty marker IS a task -->
<li><p><label><input type="checkbox" checked="" />upper</label></p></li>
<li><p>[ ]nospace</p></li>
<li><p>plain</p>
<p>[ ] task marker in second para</p></li>
</ul>
```

Key points:

- The class is `task-list`, on `<ul>` only, and **only if every item is a task**. There is no class on `<li>` or `<input>`.
- `<label>` wraps the checkbox **and the first paragraph's inlines**. It does not wrap nested lists or later blocks.
- There is **no whitespace** between `<input/>` and the text. Spacing comes entirely from CSS.
- There is **no `disabled`**, so the checkbox is clickable, though nothing persists.
- Loose items: the `<label>` sits inside the `<p>`.

`-f gfm` differs because the parser is commonmark-hs's `taskListSpec`, a separate `TaskList` block type:

```html
<ul class="task-list">
<li><label><input type="checkbox" />one</label></li>
<li><label><input type="checkbox" checked="" />two</label></li>
</ul>
<ul>
<li>plain</li>
</ul>
```

- A **mixed list is split into two lists** at the task/non-task boundary.
- **Ordered task lists are not recognized** (`1. [ ] x` stays literal), because `itemStart` uses `bulletListMarker` only.
- See `commonmark-extensions/src/Commonmark/Extensions/TaskList.hs`. Its own HTML renderer (used by `commonmark` CLI, not by pandoc) emits `class="task-list"` plus `<input type="checkbox" disabled="" ...>`.

### Pandoc version history (changelog.md) \[SRC\]

| Version | Change |
|---|---|
| 2.19 (2022-08) | "Improve CSS for task lists (#8151)". HTML writer: "Remove extra soft break for tasklist (#8142)". |
| 3.0.1 (2023-01) | "Don't disable checkboxes in task lists (#8562)". Before this, output was `<input type="checkbox" disabled="" />`. |
| 3.1.3 (2023-06) | "Use first paragraph in task item as checkbox label (#8729)". This added the `<label>` wrap. Template styles.html: "fix task-list styling in reveal.js (#8731)", which added the `[class]` specificity hack. |

### Pandoc's default CSS \[RUN\]

`pandoc --print-default-data-file templates/styles.html`, 3.13. These rules sit **outside** the `$if(document-css)$` block, so they are always emitted with the default template:

```css
/* The extra [class] is a hack that increases specificity enough to
   override a similar rule in reveal.js */
ul.task-list[class]{list-style: none;}
ul.task-list li input[type="checkbox"] {
  font-size: inherit;
  width: 0.8em;
  margin: 0 0.8em 0.2em -1.6em;
  vertical-align: middle;
}
```

- The bullet is suppressed **per list**, and only for all-task `ul`.
- A mixed list therefore renders **both a bullet and a checkbox** on its task items.
- An ordered task list shows **number + checkbox**.
- There is no nested-indent rule. Nested lists use the UA default (`padding-inline-start: 40px`).
- The checkbox hangs `-1.6em` into the gutter, and the `0.8em` right margin supplies the visual space the writer omits.

---

## 3. Quarto 1 (quarto-cli) \[SRC\]

Read from the local `external-sources/quarto-cli` at commit `d3263862a` (2026-11-03). Quarto 1 uses pandoc's writer, so the HTML is identical to §2. Its CSS overrides:

1. `src/resources/formats/html/pandoc/styles.html` is Quarto's patched partial, used by `format-html.ts` as a template partial. It sits outside `$if(document-css)$`, so it is always emitted. Commit `14ce15fa2`, "patch styles.html partial template. closes #4556":

   ```css
   ul.task-list{list-style: none;}
   ul.task-list li input[type="checkbox"] {
     width: 0.8em;
     margin: 0 0.8em 0.2em -1em; /* quarto-specific, see https://github.com/quarto-dev/quarto-cli/issues/4556 */
     vertical-align: middle;
   }
   ```

2. `src/resources/formats/html/_quarto-rules.scss`, from commit `20b3b7cc7` (2022-01-07), "fix some bootstrap list css (... task lists)":

   ```scss
   // task lists
   ul.task-list {
     padding-left: 1em;
   }
   ```

   This applies at every nesting level, against Bootstrap's `ul { padding-left: 2rem }`.

3. `src/resources/formats/revealjs/quarto.scss`, a workaround for issue #2834:

   ```scss
   .reveal ul.task-list { list-style: none; }
   .reveal ul.task-list li input[type="checkbox"] {
     width: 2em; height: 2em; margin: 0 1em 0.5em -1.6em; vertical-align: middle;
   }
   ```

4. The `gfm` format ships `github-markdown-css` (the light, dark, and auto variants under `src/resources/formats/gfm/github-markdown-css/`) for its preview.

- No Lua filter touches task lists. A grep of `src/resources/filters` for `task` found only the docusaurus extension's writer extension string.
- Net Quarto 1 HTML behavior is: all-task `ul` gets no bullets, a 1em list indent, and the checkbox at -1em, i.e. flush with the list's left padding edge. **Mixed lists and ordered lists show bullet/number + checkbox**, inherited from pandoc semantics.

Q2 context (grep only): Q2 already mirrors this. `crates/pampa/src/writers/html.rs` (`toTaskListItem` port), `crates/pampa/resources/templates/html/styles.html` (pandoc's `ul.task-list[class]` rules), the `quarto-sass` port of `ul.task-list{padding-left:1em}`, and `crates/pampa/tests/integration/test_task_list.rs` all assert that a mixed list gets no class. I did not check whether the preview path loads the `styles.html` rules.

---

## 4. JavaScript ecosystem

### 4a. markdown-it-task-lists 2.1.1 (revin) \[RUN + SRC\]

Default:

```html
<ul class="contains-task-list">
<li class="task-list-item"><input class="task-list-item-checkbox" disabled="" type="checkbox"> one</li>
<li class="task-list-item"><input class="task-list-item-checkbox" checked="" disabled="" type="checkbox"> two</li>
<li>plain</li>
</ul>
```

`{enabled:true, label:true, labelAfter:true}`:

```html
<li class="task-list-item enabled"><input class="task-list-item-checkbox"type="checkbox" id="task-item-397773"><label class="task-list-item-label" for="task-item-397773"> one</label></li>
```

- Note the missing space before `type=`. That is a real bug in the plugin's string building when `enabled`.
- The ids are random.
- With `label:true` and no `labelAfter`, a bare `<label>` wraps checkbox + text.
- Options are stored in **module-level variables**, so `.use()` with different options leaks between instances. This was observed: a later "default" render came out `enabled`.
- The marker test is `content.indexOf('[ ] ') === 0` (also `[x] ` and `[X] `). The trailing space is required, so empty `- [ ]` is not a task.
- Loose: inside `<p>`. Ordered: `ol.contains-task-list`. Mixed: the class goes on the list anyway.
- No CSS shipped.

### 4b. @hedgedoc/markdown-it-task-lists 2.0.1 \[SRC\]

Read from `dist/index.mjs`. It could not be run: it imports `markdown-it/lib/token.js`, which markdown-it 15 no longer exports.

- Regex: `/^ *\[([\sx])] /i`. Classes: `li.task-list-item` (`" enabled"` appended when enabled), `contains-task-list` on the parent list.
- Checkbox: `<input class="task-list-item-checkbox" type="checkbox" [checked=""] [disabled=""] [data-line="N"] id="task-item-N" />`, with the id taken from the source line.
- It removes `[ ] ` including the space, so there is **no space** between the input and the text.
- Optional `label` gives `<label for="task-item-N">…</label>` around the text.
- Same structure as revin's plugin, but with deterministic ids and source-line attrs for click-to-toggle.

### 4c. markdown-it-task-checkbox 1.0.6 \[RUN\]

```html
<ul class="task-list">
<li class="task-list-item"><input type="checkbox" id="cbx_0" disabled="true"><label for="cbx_0"> one</label></li>
<li class="task-list-item"><input type="checkbox" id="cbx_1" checked="true" disabled="true"><label for="cbx_1"> two</label></li>
<li>plain</li>
</ul>
```

- Uses pandoc-style `task-list` naming.
- `label for=` wraps the text only.
- `disabled="true"` is technically valid, because boolean attributes are presence-based.

### 4d. @mdit/plugin-tasklist 1.3.0 \[RUN\]

This is the plugin VitePress 2 uses, pinned at 1.0.2 in its `package.json`.

```html
<ul class="task-list-container">
<li class="task-list-item"><label class="task-list-item-label"><input type="checkbox" class="task-list-item-checkbox" disabled="disabled"> one</label></li>
<li class="task-list-item"><label class="task-list-item-label"><input type="checkbox" class="task-list-item-checkbox" checked="checked" disabled="disabled"> two</label></li>
<li>plain</li>
</ul>
```

- A `<label>` wraps the checkbox and the text (pandoc ≥3.1.3 style), with BEM-ish classes.
- Loose: inside `<p>`. Ordered: `ol.task-list-container`. Empty `[ ]`: not a task.

### 4e. remark-gfm 4.0.1 → mdast-util-to-hast 13.2.1 → rehype-stringify \[RUN + SRC\]

```html
<ul class="contains-task-list">
<li class="task-list-item"><input type="checkbox" disabled> one</li>
<li class="task-list-item"><input type="checkbox" checked disabled> two</li>
<li>plain</li>
</ul>
```

- mdast: `listItem.checked: boolean | null`, where `null` means not a task. This comes from micromark-extension-gfm-task-list-item 2.1.0 and mdast-util-gfm-task-list-item 2.0.1.
- Source `mdast-util-to-hast/lib/handlers/list-item.js`:

  ```js
  properties: {type: 'checkbox', checked: node.checked, disabled: true},
  ...
  // According to github-markdown-css, this class hides bullet.
  // See: <https://github.com/sindresorhus/github-markdown-css>.
  properties.className = ['task-list-item']
  ```

  `list.js` sets `className = ['contains-task-list']` if **any** item has a boolean `checked`.

- The checkbox is put into the first `<p>`. One is synthesized if the item has none, and in a tight list it is later unwrapped.
- There is no checkbox class and no label.
- Loose: inside `<p>`. Ordered: `ol.contains-task-list`. Empty, `nospace`, and second-paragraph markers: not tasks.

### 4f. marked 18.2.0 (GFM default) \[RUN\]

```html
<ul>
<li><input disabled="" type="checkbox"> one</li>
<li><input checked="" disabled="" type="checkbox"> two</li>
<li>plain</li>
</ul>
```

- No classes and no CSS.
- **Bug observed:** in a loose list where a *later* item has `[ ] ` at the start of a non-first paragraph, marked emitted `<input ...> [ ] x` for the first item and stripped `[ ]` from the later paragraph. Repro: `- [ ] x\n\n- plain\n\n  [ ] second`. Isolated loose lists render correctly.

### 4g. Docusaurus (main `d46582c`, 2026-11-12) \[SRC\]

MDX uses remark-gfm, so the HTML is §4e. Theme-classic overrides the `ul` MDX component in `packages/docusaurus-theme-classic/src/theme/MDXComponents/Ul/index.tsx`:

```ts
// This class is set globally by GitHub/MDX. We keep the global class, and
// add another class to get a task list without the default ul styling
// See https://github.com/syntax-tree/mdast-util-to-hast/issues/28
className?.includes('contains-task-list') && styles.containsTaskList,
```

`Ul/styles.module.css`:

```css
.containsTaskList { list-style: none; }
:not(.containsTaskList > li) > .containsTaskList { padding-left: 0; }
```

- Suppression is **per list**, which hides bullets on plain items of a mixed list too.
- A top-level task list gets `padding-left: 0`, so checkboxes sit flush at the content edge. Nested task lists keep normal padding.
- Only `ul` is overridden. Ordered task lists show number + checkbox.
- No checkbox margin rule was found in theme-classic. Infima was not checked.

### 4h. VitePress \[SRC\]

- **1.x (v1.6.4): no task-list support.** `src/node/markdown/markdown.ts` at tag `v1.6.4` has no tasklist reference.
- **2.0.0-alpha.21 (main `40b2e09`)**: enables `@mdit/plugin-tasklist` by default (`tasklist?: MarkdownItTaskListOptions | boolean`). HTML is as in §4d.
- CSS in `src/client/theme-default/styles/components/vp-doc.css`:

  ```css
  .vp-doc li.task-list-item { list-style: none; }
  .vp-doc .task-list-item-checkbox {
    margin-block: 0 0.125rem;
    margin-inline: -1.25rem 0.25rem;
    vertical-align: middle;
    accent-color: var(--vp-c-brand-1);
  }
  ```

  This is the GitHub pattern: per-item suppression, and a negative inline-start margin hanging into the gutter. It hides numbers on ordered task items.

### 4i. Astro \[SRC\]

`packages/markdown/remark` 7.3.8 enables `remark-gfm` by default (`if (gfm) parser.use(remarkGfm)`), so the HTML is as in §4e (`contains-task-list` / `task-list-item`). Astro core ships no prose CSS, which leaves styling to the theme. Starlight's CSS was not checked \[UNVERIFIED\].

---

## 5. Go / Ruby / Python / Rust / .NET

### 5a. goldmark v2.1.16 (`extension/tasklist.go`) and Hugo v0.169.0 \[SRC\]

```go
var taskListRegexp = regexp.MustCompile(`^\[([\sxX])\]\s*`)
...
if n.IsChecked { w.WriteString(`<input checked="" disabled="" type="checkbox"`) }
else           { w.WriteString(`<input disabled="" type="checkbox"`) }
// then " /> " (XHTML) or "> "
```

goldmark's own test fixture (`extension/_test/tasklist.txt`):

```html
<ul>
<li><input disabled="" type="checkbox"> foo</li>
<li><input checked="" disabled="" type="checkbox"> bar</li>
</ul>
```

- No classes.
- The parser only fires in the first text block of a `ListItem`, and only if that block has no children yet.
- The regex is `\s*`, not `\s+`. By reading, `[ ]nospace` **would** become a task, unlike GFM. This was not run \[UNVERIFIED by execution\].
- Hugo (`markup/goldmark/convert.go`) just appends `extension.TaskList` when `markup.goldmark.extensions.taskList` is true (the default). There is no list-item render hook and no CSS, so bullet handling falls to the theme.

### 5b. Jekyll: kramdown 2.5.2 + kramdown-parser-gfm 1.1.0 \[RUN + SRC\]

Jekyll's default config sets `kramdown: input: "GFM"` (`lib/jekyll/configuration.rb`).

```html
<ul class="task-list">
  <li class="task-list-item"><input type="checkbox" class="task-list-item-checkbox" disabled="disabled" />one</li>
  <li class="task-list-item"><input type="checkbox" class="task-list-item-checkbox" disabled="disabled" checked="checked" />two</li>
  <li class="task-list-item">plain</li>
</ul>
```

- Note `li.task-list-item` on **"plain"** too. The source is `parse_list` in `kramdown/parser/gfm.rb`:

  ```ruby
  checked    = descendant.gsub!(/\A\s*\[ \]\s+/,  box_unchecked)
  unchecked  = descendant.gsub!(/\A\s*\[x\]\s+/i, box_checked)
  is_tasklist ||= checked || unchecked
  li.attr['class'] = 'task-list-item' if is_tasklist     # sticky: every li AFTER the first task
  ...
  current_list.attr['class'] = 'task-list' if is_tasklist
  ```

  The flag is sticky, so every `<li>` after the first task gets the class.

- The `\s+` is consumed, so there is **no space** between the input and the text.
- Loose: inside `<p>`. Ordered: `ol.task-list`. Empty `[ ]`, `nospace`, and second-paragraph markers: not tasks.
- Nested items in the tight sub-list are handled the same way.

minima theme CSS:

- The released minima **2.5.1** has **no** task-list CSS (0 matches in `_sass/minima/_layout.scss` at tag `v2.5.1`).
- minima master (`4de322363f`, 2026-04, the unreleased 3.x) `_sass/minima/_layout.scss` has:

  ```scss
  .task-list { margin-left: 0; padding-left: $spacing-unit * 0.6; }
  .task-list-item {
    list-style-type: none;
    &-checkbox {
      position: relative;
      margin-right: $spacing-unit * 0.3;
      margin-left: -$spacing-unit * 0.5;
      appearance: none;
      border: 8px solid $border-color-01;
      vertical-align: text-top;
      z-index: -1;
      &::after { /* custom checkmark */ }
  ```

### 5c. Python-Markdown 3.11.3 + pymdown-extensions 13.1 `pymdownx.tasklist` \[RUN + SRC\]

This is what MkDocs Material uses. Python-Markdown core has **no** task-list support.

Default:

```html
<ul class="task-list">
<li class="task-list-item"><input type="checkbox" disabled/> one</li>
<li class="task-list-item"><input type="checkbox" disabled checked/> two</li>
<li>plain</li>
</ul>
```

`custom_checkbox: true, clickable_checkbox: true`:

```html
<li class="task-list-item"><label class="task-list-control"><input type="checkbox" checked/><span class="task-list-indicator"></span></label> two</li>
```

- `clickable_checkbox` just drops `disabled`.
- `custom_checkbox` wraps **only the input + an empty indicator span** in the label. The item text is outside the label, so the label has no text content.
- Source `RE_CHECKBOX = r"^(?P<checkbox> *\[(?P<state>(?:x|X| ){1})\] +)(?P<line>.*)"`. It checks `li.text`, or the first child `<p>` for loose items. `task-list` is added to the parent if **any** child is a task.
- With 4-space-indent fixtures (Python-Markdown is not CommonMark): ordered gives `ol.task-list`, loose items are inside `<p>`, and empty/nospace items are not tasks.

MkDocs Material CSS (`src/templates/assets/stylesheets/main/extensions/pymdownx/_tasklist.scss`, master, release 9.8.1):

```scss
.md-typeset {
  .task-list-item {
    position: relative;
    list-style-type: none;
    [type="checkbox"] {
      position: absolute;
      inset-inline-start: -2em;
      top: 0.45em;
    }
  }
  .task-list-control [type="checkbox"] { z-index: -1; opacity: 0; }
  .task-list-indicator::before {
    position: absolute;
    inset-inline-start: px2em(-24px);
    top: 0.15em;
    width: px2em(20px); height: px2em(20px);
    content: "";
    background-color: var(--md-default-fg-color--lightest);
    mask-image: var(--md-tasklist-icon);
    ...
  }
  [type="checkbox"]:checked + .task-list-indicator::before {
    background-color: $clr-green-a400;
    mask-image: var(--md-tasklist-icon--checked);
  }
}
```

- Suppression is **per item**.
- The checkbox is **absolutely positioned into the marker gutter** (`-2em`), so the text column is identical to ordinary list items.
- Custom mode hides the native input (opacity 0) and paints an icon.

### 5d. markdown-it-py 4.3.0 + mdit-py-plugins 0.7.0 `tasklists`, and MyST (myst-parser 5.2.0) \[RUN\]

mdit-py-plugins is a port of revin's plugin:

```html
<ul class="contains-task-list">
<li class="task-list-item"><input class="task-list-item-checkbox" disabled="disabled" type="checkbox"> one</li>
<li class="task-list-item"><input class="task-list-item-checkbox" checked="checked" disabled="disabled" type="checkbox"> two</li>
<li>plain</li>
</ul>
```

With `enabled=True, label=True`:

```html
<li class="task-list-item enabled"><label><input class="task-list-item-checkbox"  type="checkbox"> one</label></li>
```

MyST via `myst-docutils-html5 --myst-enable-extensions=tasklist` (the docutils/Sphinx path):

```html
<ul class="contains-task-list simple">
<li class="task-list-item"><p><input class="task-list-item-checkbox" disabled="disabled" type="checkbox">one</p></li>
<li class="task-list-item"><p><input class="task-list-item-checkbox" checked="checked" disabled="disabled" type="checkbox">two</p></li>
<li><p>plain</p></li>
</ul>
```

- Docutils always wraps item text in `<p>`. The `simple` class marks tight lists.
- There is **no space** between the input and the text.
- Ordered: `ol class="arabic simple contains-task-list"`.
- myst-parser ships **no** task-list CSS (no `.css` in the package), so styling falls to the Sphinx theme. Themes were not surveyed.

### 5e. pulldown-cmark 0.13.6 (`ENABLE_TASKLISTS`) and Zola v0.23.6 \[RUN + SRC\]

```html
<ul>
<li><input disabled="" type="checkbox"/>
one</li>
<li><input disabled="" type="checkbox" checked=""/>
two</li>
<li>plain</li>
</ul>
```

Source `pulldown-cmark/src/html.rs`:

```rust
TaskListMarker(true)  => self.write("<input disabled=\"\" type=\"checkbox\" checked=\"\"/>\n")?,
TaskListMarker(false) => self.write("<input disabled=\"\" type=\"checkbox\"/>\n")?,
```

- A **newline**, not a space, separates the input and the text. It renders as one space.
- No classes.
- Loose: inside `<p>`. Ordered: yes.
- **Empty `- [ ]` IS a task** (`<li><input disabled="" type="checkbox"/>\n</li>`). `nospace` is not.
- Zola sets `opts.insert(Options::ENABLE_TASKLISTS)` unconditionally (`components/markdown/src/context.rs`) and uses pulldown's HTML. There is no class and no CSS; it is theme-dependent.

### 5f. comrak 0.59.0 \[RUN\]

Default (cmark-gfm-compatible):

```html
<ul>
<li><input type="checkbox" disabled="" /> one</li>
<li><input type="checkbox" checked="" disabled="" /> two</li>
<li>plain</li>
</ul>
```

With `render.tasklist_classes = true`:

```html
<ul class="contains-task-list">
<li class="task-list-item"><input type="checkbox" class="task-list-item-checkbox" disabled="" /> one</li>
<li class="task-list-item"><input type="checkbox" class="task-list-item-checkbox" checked="" disabled="" /> two</li>
<li>plain</li>
</ul>
```

- Loose: the checkbox goes **before** `<p>` (cmark-gfm behavior), e.g. `<li ...><input ... /> \n<p>loose one</p>`.
- **Empty `- [ ]` IS a task.**
- Ordered gets `ol.contains-task-list`, and mixed lists get the class.
- Other options exist (`relaxed_tasklist_matching`, `tasklist_in_table`, and in GitLab's usage `inapplicable_tasks`).

### 5g. Markdig v0.16.0 tag listing (master) \[SRC\]

`src/Markdig/Extensions/TaskLists/`.

```csharp
// TaskListInlineParser
ListClass = "contains-task-list";
ListItemClass = "task-list-item";
...
if (!c.IsSpace() && c != 'x' && c != 'X') return false;
if (slice.NextChar() != ']') return false;
...
listItemBlock.GetAttributes().AddClass(ListItemClass);
listBlock.GetAttributes().AddClass(ListClass);

// HtmlTaskListRenderer
renderer.Write("<input").WriteAttributes(obj).Write(" disabled=\"disabled\" type=\"checkbox\"");
if (obj.Checked) renderer.Write(" checked=\"checked\"");
renderer.Write(" />");
```

Markdig's own spec (`src/Markdig.Tests/Specs/TaskListSpecs.md`):

```html
<ul class="contains-task-list">
<li class="task-list-item"><input disabled="disabled" type="checkbox" /> Item1</li>
<li class="task-list-item"><input disabled="disabled" type="checkbox" checked="checked" /> Item2</li>
<li class="task-list-item"><input disabled="disabled" type="checkbox" /> Item3</li>
<li>Item4</li>
</ul>
```

- Mixed lists get the class. "Ordered or unordered" is stated in the spec.
- The parser is **inline**. The shown `Match` checks only that the inline's block's parent is a `ListItemBlock`, and does not require trailing whitespace. By reading, `[ ]nospace` and possibly a marker at the start of a second paragraph could match. \[UNVERIFIED by execution; there may be a position check elsewhere.\]

---

## 6. GitLab and Obsidian

### GitLab \[SRC, partially UNVERIFIED\]

From the GitHub mirror `gitlabhq/gitlabhq` at `d7b9a6176a` (2026-10-23):

- `lib/banzai/filter/markdown_engines/glfm_markdown.rb` renders with comrak (the `gitlab-glfm-markdown` gem) using `tasklist: true, tasklist_classes: true, tasklist_in_table: true, relaxed_tasklist_character: true, inapplicable_tasks: true`. That suggests comrak's `contains-task-list`/`task-list-item`/`task-list-item-checkbox` markup (§5f). However, GitLab's CSS targets `ul.task-list`, so the exact list class in GitLab's fork is **unverified**. The API requires auth, so it could not be rendered live.
- `lib/banzai/filter/task_list_filter.rb` post-processes every `input.task-list-item-checkbox`:
  - It sets `aria-label` to `"Check option: <item text, truncated to 100 chars>"`, or `"Check option in column \"…\""` for table tasks, with `"Check option"` as the fallback.
  - It wraps the text of `[~]` "inapplicable" items in `<s class="inapplicable">`.
- CSS (`app/assets/stylesheets/framework/typography.scss`):

  ```scss
  ul.task-list, ol.task-list {
    > li.task-list-item {
      position: relative; min-height: 22px; padding-inline-start: 32px;
      > p > input.task-list-item-checkbox,
      > input.task-list-item-checkbox {
        position: absolute; inset-inline-start: $gl-padding-8; inset-block-start: 3px;
      }
    }
  }
  ul.task-list > li.task-list-item { list-style-type: none; margin-inline-start: 0 !important; }
  ul.task-list .task-list-item-checkbox, ol.task-list .task-list-item-checkbox { all: unset; /* custom box */ ... }
  ```

  - Bullets are suppressed per item, **only for `ul`**. Ordered task items keep their numbers.
  - The checkbox is absolutely positioned inside the li's own padding.
  - The CSS handles both checkbox placements: directly in `li` and inside `li > p`.
  - Checkboxes are interactive (toggle by `data-sourcepos`, per the GitLab 11.8 blog post "more performant and robust task lists").

### Obsidian \[UNVERIFIED, closed source\]

The following comes from community docs: the Obsidian Tasks plugin styling page (https://publish.obsidian.md/tasks/Advanced/Styling) and forum CSS snippets.

- Reading view uses `ul.contains-task-list`, `li.task-list-item` (plus `is-checked` when done, and `data-task="<char>"` for custom statuses like `[-]`, `[/]`, `[>]`), and `input.task-list-item-checkbox` with `data-line`. Checkboxes are **clickable** and write back to the source.
- Checked items are struck through by default theme CSS.
- Live Preview uses different CodeMirror DOM.
- UX takeaway: the checkbox **replaces** the bullet, any character in `[ ]` is accepted as a status, and toggling is first-class.

---

## 7. Comparison tables

### 7a. Markup

| Renderer (version) | list class | li class | input class | label | disabled | input→text whitespace | evidence |
|---|---|---|---|---|---|---|---|
| GFM spec (0.29-gfm) | none | none | none | no | `disabled=""` | 1 space | SRC |
| cmark-gfm 0.29.0.gfm.13 | none | none | none | no | yes | 1 space | RUN |
| github.com (2026-11) | `contains-task-list` | `task-list-item` | `task-list-item-checkbox` (+`aria-label`, `id=""`) | no | yes | 1 space | RUN |
| pandoc 3.13 `markdown` | `task-list` (ul, all-task only) | none | none | **wraps input + first para text** | **no** (since 3.0.1) | **none** | RUN |
| pandoc 3.13 `gfm` | `task-list` (splits mixed lists) | none | none | wraps | no | none | RUN |
| Quarto 1 | = pandoc | = pandoc | = pandoc | = pandoc | = pandoc | none | SRC |
| markdown-it-task-lists 2.1.1 | `contains-task-list` | `task-list-item` (`enabled` opt) | `task-list-item-checkbox` | opt (`label`, `labelAfter`) | yes (opt off) | 1 space | RUN |
| @hedgedoc/markdown-it-task-lists 2.0.1 | `contains-task-list` | `task-list-item` | `task-list-item-checkbox` (+`id`, `data-line`) | opt (`for=`) | yes (opt off) | none | SRC |
| markdown-it-task-checkbox 1.0.6 | `task-list` | `task-list-item` | none (`id`) | text only, `for=` | `disabled="true"` | 1 space (in label) | RUN |
| @mdit/plugin-tasklist 1.3.0 (VitePress 2) | `task-list-container` | `task-list-item` | `task-list-item-checkbox` | **wraps input + text** (`task-list-item-label`) | yes | 1 space | RUN |
| remark-gfm 4 / mdast-util-to-hast 13 (Docusaurus, Astro) | `contains-task-list` | `task-list-item` | none | no | yes | 1 space | RUN |
| marked 18.2.0 | none | none | none | no | yes | 1 space | RUN |
| goldmark 2.1.16 / Hugo 0.169 | none | none | none | no | yes | 1 space | SRC |
| kramdown-parser-gfm 1.1.0 (Jekyll) | `task-list` | `task-list-item` (sticky, also on later plain items) | `task-list-item-checkbox` | no | `disabled="disabled"` | none | RUN |
| pymdownx.tasklist 13.1 (MkDocs) | `task-list` | `task-list-item` | none (custom: `label.task-list-control`, `span.task-list-indicator`) | custom mode: input only | yes (opt `clickable_checkbox`) | 1 space | RUN |
| mdit-py-plugins 0.7 / MyST 5.2 | `contains-task-list` | `task-list-item` | `task-list-item-checkbox` | opt | yes | 1 space (MyST/docutils: none) | RUN |
| pulldown-cmark 0.13.6 / Zola | none | none | none | no | yes | newline | RUN |
| comrak 0.59 | none / `contains-task-list` (opt) | none / `task-list-item` | none / `task-list-item-checkbox` | no | yes | 1 space | RUN |
| Markdig (master) | `contains-task-list` | `task-list-item` | none | no | `disabled="disabled"` | 1 space | SRC |
| GitLab | `task-list`? (CSS targets it) | `task-list-item` | `task-list-item-checkbox` + `aria-label="Check option: …"` | no | interactive | ? | SRC/UNVERIFIED |
| Obsidian | `contains-task-list` | `task-list-item` (+`is-checked`), `data-task` | `task-list-item-checkbox` | ? | interactive | ? | UNVERIFIED |

### 7b. Semantics and edge cases

✓ = becomes a task / gets the class; ✗ = does not; — = not applicable or not tested.

| Renderer | mixed list gets list class | ordered tasks | ordered list class | loose: checkbox placement | empty `- [ ]` | `[X]` | `[ ]nospace` | marker in 2nd para |
|---|---|---|---|---|---|---|---|---|
| github.com | ✓ | ✓ | ✓ | inside `<p>` | ✗ | ✓ | ✗ | ✗ |
| cmark-gfm | — | ✓ | — | **before `<p>`** | ✗ | ✓ | ✗ | ✗ |
| pandoc `markdown` | **✗ (all-or-nothing)** | ✓ | **✗** | inside `<p>` (in `<label>`) | **✓** | ✓ | ✗ | ✗ |
| pandoc `gfm` | **splits the list** | **✗** | — | inside `<p>` | ✓ | ✓ | ✗ | ✗ |
| markdown-it-task-lists | ✓ | ✓ | ✓ | inside `<p>` | ✗ | ✓ | ✗ | ✗ |
| @mdit/plugin-tasklist | ✓ | ✓ | ✓ | inside `<p>` | ✗ | ✓ | ✗ | ✗ |
| remark-gfm | ✓ | ✓ | ✓ | inside `<p>` | ✗ | ✓ | ✗ | ✗ |
| marked | — | ✓ | — | inside `<p>` (buggy, see §4f) | ✗ | ✓ | ✗ | ✗ |
| kramdown GFM | ✓ | ✓ | ✓ | inside `<p>` | ✗ | ✓ | ✗ | ✗ |
| pymdownx.tasklist | ✓ | ✓ | ✓ | inside `<p>` | ✗ | ✓ | ✗ | ✗ |
| mdit-py / MyST | ✓ | ✓ | ✓ | inside `<p>` | ✗ | ✓ | ✗ | ✗ |
| pulldown-cmark | — | ✓ | — | inside `<p>` | **✓** | ✓ | ✗ | ✗ |
| comrak | ✓ (opt) | ✓ | ✓ (opt) | **before `<p>`** | **✓** | ✓ | ✗ | ✗ |
| goldmark | — | ✓ (by reading) | — | — | — | ✓ | ✓? (regex `\s*`) \[UNVERIFIED\] | ✗ |
| Markdig | ✓ | ✓ | ✓ | — | — | ✓ | ✓? \[UNVERIFIED\] | ? |

### 7c. Shipped CSS: who hides the marker, and how the checkbox is placed

| Project | hides marker on | mixed list result | ordered task items | checkbox placement | nested |
|---|---|---|---|---|---|
| GitHub (`github-markdown-css`) | `li.task-list-item` | bullets only on plain items | numbers hidden | margin `0 .2em .25em -1.4em` (hangs into the 2em gutter) | normal 2em padding per level |
| pandoc `styles.html` | `ul.task-list[class]` | **bullet + checkbox** (no class) | **number + checkbox** | `width:.8em; margin: 0 .8em .2em -1.6em` | UA default padding |
| Quarto 1 | `ul.task-list` | bullet + checkbox | number + checkbox | `margin: 0 .8em .2em -1em`; `ul.task-list{padding-left:1em}` | 1em at each task-list level |
| Docusaurus | `ul.contains-task-list` (via CSS module) | **all bullets hidden** | number + checkbox | no rule found | top-level `padding-left:0`, nested keeps padding |
| VitePress 2 | `li.task-list-item` | bullets only on plain items | numbers hidden | `margin-inline: -1.25rem .25rem` | theme default |
| MkDocs Material | `li.task-list-item` | bullets only on plain items | numbers hidden | `position:absolute; inset-inline-start:-2em` | theme default |
| minima (master only) | `li.task-list-item` | bullets only on plain items | numbers hidden | relative + negative margin, custom box | `.task-list` padding 0.6×unit |
| GitLab | `ul.task-list > li.task-list-item` | bullets only on plain items | **numbers kept**, checkbox absolute in li padding | `position:absolute` inside the 32px li padding | — |
| cmark-gfm, comrak, pulldown/Zola, goldmark/Hugo, marked, remark/Astro, MyST | nothing shipped (theme's job) | bullet + checkbox | number + checkbox | — | — |

### 7d. Accessibility and interactivity

- **Label association.** Pandoc ≥3.1.3 and @mdit/plugin-tasklist wrap input+text in `<label>`, giving the checkbox its text as accessible name and making the text a click target. markdown-it-task-lists, hedgedoc, and markdown-it-task-checkbox can use `label for=`, opt-in except task-checkbox. GitHub uses `aria-label="Completed task"`/`"Incomplete task"`, which is a state-only name without the text. GitLab uses `aria-label="Check option: <text>"`. Everyone else emits an unlabeled checkbox. pymdownx custom mode wraps only the input, so its label has no text.
- **Disabled.** Nearly every renderer emits `disabled` by default, as GFM's examples do. **Pandoc ≥3.0.1 is the outlier**: its checkboxes are enabled but non-persistent (#8562). Interactive behavior is opt-in elsewhere: markdown-it `enabled`, pymdownx `clickable_checkbox`, hedgedoc. It is app-level in GitHub, GitLab, and Obsidian.

---

## 8. Convergence / divergence synthesis

### What nearly everyone does

1. **The checkbox replaces the marker visually.** Every project that ships CSS hides the bullet for task items, and nobody intends a bullet *and* a checkbox. Where both show (pandoc and Quarto 1 mixed or ordered lists, and every unstyled renderer), it is a gap in the CSS, not a design goal.
2. **The checkbox hangs into the marker gutter.** Text stays aligned with sibling and non-task item text, either through a negative inline-start margin (GitHub −1.4em, pandoc −1.6em, VitePress −1.25rem, Quarto −1em) or through absolute positioning (Material −2em, GitLab). List padding is otherwise unchanged. Quarto 1 and Docusaurus are the exceptions: they *reduce* the list padding.
3. **`<input type="checkbox">` comes first in the first paragraph**, inside `<p>` for loose items (GitHub, pandoc, remark, markdown-it family, kramdown, pymdownx, MyST, pulldown). Only cmark-gfm and comrak put it before the `<p>`.
4. **`[x]` and `[X]` both mean checked.** A marker in a non-first paragraph is never a task. `[ ]` with no following whitespace is not a task in any renderer actually run.
5. **Disabled by default**, with pandoc as the exception.
6. **Class vocabulary.** GitHub's `contains-task-list` / `task-list-item` / `task-list-item-checkbox` is the de-facto standard: GitHub, remark (Docusaurus, Astro), markdown-it-task-lists, hedgedoc, mdit-py and MyST, Markdig, comrak, Obsidian. The older `task-list` name survives in pandoc, kramdown/Jekyll, pymdownx/MkDocs, and GitLab CSS. Several, including kramdown and pymdownx, also use `li.task-list-item` and `input.task-list-item-checkbox`.

### Where they disagree

1. **Mixed lists, which is the big one.** Most put the list class on any list containing ≥1 task and mark task `<li>`s individually. CSS then hides markers **per item**: GitHub, VitePress, Material, minima, GitLab. Pandoc's HTML writer is the strict outlier: the class goes on the list **only if all items are tasks**, and pandoc puts **no class on `<li>`**. Its CSS can therefore only act per list, so mixed lists get bullet + checkbox. pandoc's `gfm` reader splits mixed lists into separate lists instead. Docusaurus goes the other way: it hides all bullets in any list with a task.
2. **Ordered lists.** Everyone parses ordered task items except pandoc's gfm/commonmark-hs reader. Pandoc's writer gives `<ol>` no class. Per-`li` CSS (GitHub, VitePress, Material) hides the numbers. GitLab deliberately keeps them, and pandoc and Quarto keep them by omission.
3. **Label wrapping.** Pandoc ≥3.1.3 and @mdit wrap input+text in `<label>`, which is the most accessible option. GitHub uses a state-only `aria-label`. Most emit nothing.
4. **Whitespace between checkbox and text.** It is one literal space in most renderers. Pandoc, kramdown, MyST, and hedgedoc emit none and rely on CSS margin. pulldown-cmark emits a newline. A renderer without spacing CSS therefore looks cramped.
5. **Empty `- [ ]`.** Pandoc, pulldown-cmark, and comrak make it a task. GitHub, cmark-gfm, remark, markdown-it, kramdown, pymdownx, and MyST do not.
6. **Loose-list placement.** Most put the checkbox inside `<p>`. cmark-gfm and comrak put it before `<p>`. GitLab's CSS handles both cases.
7. **Nested indentation.** No consensus. Most leave nesting to normal list padding. Quarto 1 uses 1em per task-list level, and Docusaurus zeroes only the top level.

### Implication for Q2 (observations, not a decision)

The "bullet + checkbox" symptom is exactly what pandoc and Quarto 1 CSS produce in two cases: (a) a mixed list, because the writer gives it no `task-list` class, and (b) any page where pandoc's `styles.html` task-list rules are not loaded. Fixing it as the ecosystem does would mean emitting a per-item hook (`li.task-list-item`, or targeting `li:has(> input[type=checkbox])` / `li:has(> p:first-child > label > input)`) and hiding markers per item. That diverges from pandoc's markup, so it is a design choice to make deliberately.

## Sources

- GFM spec: <https://github.github.com/gfm/> ; <https://github.com/github/cmark-gfm/blob/master/test/spec.txt> ; <https://github.com/github/cmark-gfm/blob/master/extensions/tasklist.c>
- github.com: live `gh api /markdown` (2026-11-12); <https://github.com/sindresorhus/github-markdown-css> (commit 7f38dcb905); <https://github.com/github-archive/task_list/blob/master/lib/task_list/filter.rb>
- Pandoc: pandoc 3.13 binary; <https://github.com/jgm/pandoc/blob/main/src/Text/Pandoc/Writers/HTML.hs> ; <https://github.com/jgm/pandoc/blob/main/changelog.md> ; <https://github.com/jgm/commonmark-hs/blob/master/commonmark-extensions/src/Commonmark/Extensions/TaskList.hs>
- Quarto 1: quarto-cli @ d3263862a, files cited in §3
- JS: npm packages at the versions given; <https://github.com/syntax-tree/mdast-util-to-hast> (lib/handlers/list-item.js, list.js); Docusaurus @ d46582c; VitePress @ 40b2e09 and tag v1.6.4; <https://github.com/withastro/astro/blob/main/packages/markdown/remark/src/index.ts>
- Go/Ruby/Python/Rust/.NET: <https://github.com/yuin/goldmark/blob/master/extension/tasklist.go> ; <https://github.com/gohugoio/hugo/blob/master/markup/goldmark/convert.go> ; kramdown-parser-gfm 1.1.0 gem `lib/kramdown/parser/gfm.rb`; <https://github.com/jekyll/jekyll/blob/master/lib/jekyll/configuration.rb> ; <https://github.com/jekyll/minima> (master and v2.5.1); pymdown-extensions 13.1 `pymdownx/tasklist.py`; <https://github.com/squidfunk/mkdocs-material/blob/master/src/templates/assets/stylesheets/main/extensions/pymdownx/_tasklist.scss> ; mdit-py-plugins 0.7.0 `tasklists/__init__.py`; myst-parser 5.2.0 `mdit_to_docutils/base.py`; <https://github.com/pulldown-cmark/pulldown-cmark/blob/master/pulldown-cmark/src/html.rs> ; <https://github.com/getzola/zola/blob/master/components/markdown/src/context.rs> ; comrak 0.59.0 crate; <https://github.com/xoofx/markdig/tree/master/src/Markdig/Extensions/TaskLists>
- GitLab: <https://github.com/gitlabhq/gitlabhq> (lib/banzai/filter/task_list_filter.rb, lib/banzai/filter/markdown_engines/glfm_markdown.rb, app/assets/stylesheets/framework/typography.scss) @ d7b9a6176a; <https://gitlab.com/gitlab-org/gitlab/-/issues/357684> ; <https://about.gitlab.com/blog/more-performant-and-robust-task-lists-in-gitlab/>
- Obsidian: <https://publish.obsidian.md/tasks/Advanced/Styling> ; <https://forum.obsidian.md/t/reading-mode-dont-strike-through-nested-checkboxes-when-outer-checkbox-is-checked/8389> ; <https://zerokspot.com/weblog/2026/07/05/additional-checkbox-styles-in-obsidian/>

Local harnesses (scratchpad): fixtures in `t/`, runner scripts in `scripts/` (`pyr.py`, `jsr.mjs`, `mdit.mjs`, `one.mjs`), Rust harness in `rs/`, fetched sources in `src/`.
