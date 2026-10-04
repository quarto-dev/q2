-- ooxml-editorial-marks.lua
--
-- Rewrites Quarto's editorial-mark Spans and Divs for docx and pptx output.
-- Input nodes are Spans (inline) or Divs (block) whose first class is one of
-- quarto-insert, quarto-delete, quarto-highlight or quarto-edit-comment; id,
-- further classes and key-value attributes (e.g. author=, date=) follow.
-- See README.md for the background and the reasoning behind this design.
--
-- Layout:
--   * Shared helpers: class rewriting and Div -> Span wrapping.
--   * docx handlers: rewrite each mark into the class vocabulary pandoc's
--     docx writer turns into native OOXML (insertion, deletion, mark,
--     comment-start/comment-end).
--   * pptx handlers: replace each mark with one flattened raw <a:r> run.
--   * Dispatch: one handler table per format, selected by FORMAT and keyed
--     by the node's first class.
--
-- Handlers are called by a single topdown traversal over Span and Div. Each
-- handler returns its replacement, plus `false` where traversal must not
-- continue into the replacement. A handler that returns a list of nodes
-- cannot rely on the traversal to visit the list's elements, so anything
-- nested that needs converting is dispatched explicitly (see
-- wrap_blocks_with_span); comments rely on the opposite, keeping their
-- original content unvisited so that nested marks stay plain text.

if not (FORMAT == "docx" or FORMAT == "pptx") then
  return {}
end

local function first_class(el)
  return el.classes[1]
end

local QUARTO_MARK_CLASSES = {
  ["quarto-insert"] = true,
  ["quarto-delete"] = true,
  ["quarto-highlight"] = true,
  ["quarto-edit-comment"] = true,
}

local function is_quarto_mark(block)
  return block.classes and QUARTO_MARK_CLASSES[block.classes[1]]
end

-- Forward declaration: assigned once the docx/pptx dispatch tables exist
-- (bottom of file). Needed here because block-wrapping helpers below must
-- recursively re-dispatch a nested quarto-* mark themselves: pandoc's
-- `traverse = "topdown"` walk does NOT re-descend into a `Blocks`/`Inlines`
-- *list* returned as a replacement (verified empirically -- only a
-- single-node replacement gets walked further), so relying on the walker to
-- separately visit a nested mark left untouched inside such a list would
-- silently skip it.
local dispatch_element

--------------------------------------------------------------------------
-- Shared helpers
--------------------------------------------------------------------------

-- Replace `el`'s first class (the quarto-* marker) with `new_class`,
-- keeping id, remaining user classes, and all attributes untouched.
local function with_first_class_replaced(el, new_class)
  local classes = {}
  for i, c in ipairs(el.classes) do
    if i == 1 then
      classes[i] = new_class
    else
      classes[i] = c
    end
  end
  local new_el = el:clone()
  new_el.classes = classes
  return new_el
end

-- Wrap every paragraph-like block's inline content, at any depth, in a Span
-- with `attr`. Used to bring block-level (Div) marks down to the run level,
-- since pandoc's docx writer only special-cases Span classes. A nested
-- quarto-* mark is converted to its own native form directly (and not
-- descended into): the walk below would otherwise wrap its paragraphs too.
-- CodeBlocks have no run-level home for the marker and are left as-is.
local function wrap_blocks_with_span(blocks, attr)
  local function wrap(block)
    local new_block = block:clone()
    new_block.content = pandoc.Inlines({ pandoc.Span(block.content, attr) })
    return new_block, false
  end
  return pandoc.Blocks(blocks):walk({
    traverse = "topdown",
    Div = function(div)
      if is_quarto_mark(div) then
        return dispatch_element(div), false
      end
    end,
    Para = wrap,
    Plain = wrap,
    Header = wrap,
  })
end

--------------------------------------------------------------------------
-- docx handlers
--------------------------------------------------------------------------

-- Attr for the Span that stands in for a block-level insertion/deletion:
-- the writer reads w:author / w:date from author= / date= on the Span, so
-- those are carried over from the Div.
local function track_change_attr(el, class)
  local kvs = {}
  for _, key in ipairs({ "author", "date" }) do
    if el.attributes[key] then
      table.insert(kvs, { key, el.attributes[key] })
    end
  end
  return pandoc.Attr("", { class }, kvs)
end

--- Emit a Word `<w:ins>` run (inline) or wrap each paragraph's runs in one
--- (block), via pandoc's native "insertion" class support. w:author/w:date
--- come from el.attributes.author / .date when present (pandoc defaults
--- w:author to "unknown" itself when absent); w:id is auto-numbered by
--- pandoc, so no id management is needed here.
local function docx_handle_insert(el)
  if el.t == "Div" then
    return wrap_blocks_with_span(el.content, track_change_attr(el, "insertion"))
  end
  return with_first_class_replaced(el, "insertion")
end

--- Emit a Word `<w:del>`/`<w:delText>` run, via pandoc's native "deletion"
--- class support. Deleted content is not rendered as ordinary visible text
--- once opened in Word with track changes displayed.
local function docx_handle_delete(el)
  if el.t == "Div" then
    return wrap_blocks_with_span(el.content, track_change_attr(el, "deletion"))
  end
  return with_first_class_replaced(el, "deletion")
end

--- Emit a Word run with `<w:highlight w:val="yellow"/>` shading, via
--- pandoc's native (narrowly-matched) bare ("", {"mark"}, {}) Span support.
--- id / extra classes / extra kv attributes are intentionally dropped: the
--- writer only applies the highlight when the Span carries nothing else,
--- and there is no OOXML equivalent for arbitrary metadata on a highlighted
--- run.
local function docx_handle_highlight(el)
  local attr = pandoc.Attr("", { "mark" }, {})
  if el.t == "Div" then
    return wrap_blocks_with_span(el.content, attr)
  end
  return pandoc.Span(el.content, attr)
end

local comment_counter = 0
local function next_comment_id(el)
  if el.identifier and el.identifier ~= "" then
    return el.identifier
  end
  comment_counter = comment_counter + 1
  return "quarto-comment-" .. tostring(comment_counter)
end

-- Only author/date have an OOXML home on <w:comment> (plus w:initials,
-- which quarto-edit-comment marks don't carry); other kv attributes (e.g.
-- a stray `key="value"`) have nowhere valid to go and are dropped.
local function comment_attrs(el, id)
  local attrs = { { "id", id } }
  local author = el.attributes["author"]
  local date = el.attributes["date"]
  if author then table.insert(attrs, { "author", author }) end
  if date then table.insert(attrs, { "date", date }) end
  return attrs
end

--- Emit an OOXML comment: a zero-width `<w:commentRangeStart>`/
--- `<w:commentRangeEnd>` + `<w:commentReference>` pair, via pandoc's native
--- "comment-start"/"comment-end" class support, plus the corresponding
--- `<w:comment>` entry in word/comments.xml (content-type and relationship
--- registrations are generated by pandoc).
---
--- For an inline mark (`[>> text]`) with no separate anchor, the mark's own
--- content becomes the comment's message and nothing is left visible in the
--- body -- matching Word's default "comment on the cursor position, no
--- selection" appearance.
---
--- For a block mark (`::: >> ... :::`), the div's own content is both the
--- comment's message *and* stays visible in the body (per
--- document_profile.rs: "there is no separate target content" -- the
--- paragraph is its own anchor), wrapped with the range markers at its
--- first/last paragraph.
---
--- Returns `false` as a second value to stop descent: a comment nested
--- inside another comment must fold into the outer comment's text rather
--- than becoming an independent Word comment (see
--- `profile_extract_block_comment_is_a_leaf`). Reusing the original,
--- not-yet-recursed content as the comment-start Span's message achieves
--- this for free -- any nested quarto-* mark inside is left with its
--- original (writer-unrecognized) class and simply renders as plain text.
local function docx_handle_edit_comment(el)
  local id = next_comment_id(el)
  local attrs = comment_attrs(el, id)

  if el.t == "Span" then
    local start_span = pandoc.Span(el.content, pandoc.Attr("", { "comment-start" }, attrs))
    local end_span = pandoc.Span({}, pandoc.Attr("", { "comment-end" }, { { "id", id } }))
    return pandoc.Inlines({ start_span, end_span }), false
  end

  -- Div: the message is the flattened div content; the body keeps the
  -- original (unwrapped) blocks, with range markers spliced onto the
  -- first/last paragraph.
  local message = pandoc.utils.blocks_to_inlines(el.content)
  local start_span = pandoc.Span(message, pandoc.Attr("", { "comment-start" }, attrs))
  local end_span = pandoc.Span({}, pandoc.Attr("", { "comment-end" }, { { "id", id } }))

  local blocks = {}
  for i, b in ipairs(el.content) do blocks[i] = b end
  if #blocks == 0 then
    return pandoc.Blocks({ pandoc.Para({ start_span, end_span }) }), false
  end

  -- Put `span` at the start or end of the first/last block's inlines, or in
  -- a paragraph of its own when that block has no inlines (e.g. a CodeBlock).
  local function attach(idx, span, at_start)
    local blk = blocks[idx]
    if blk.t == "Para" or blk.t == "Plain" or blk.t == "Header" then
      local content = pandoc.Inlines({})
      if at_start then content:insert(span) end
      content:extend(blk.content)
      if not at_start then content:insert(span) end
      blocks[idx] = blk:clone()
      blocks[idx].content = content
    elseif at_start then
      table.insert(blocks, 1, pandoc.Para({ span }))
    else
      table.insert(blocks, pandoc.Para({ span }))
    end
  end
  attach(1, start_span, true)
  attach(#blocks, end_span, false)

  return pandoc.Blocks(blocks), false
end

--------------------------------------------------------------------------
-- pptx fallback handlers
--------------------------------------------------------------------------

local function xml_escape(s)
  return (s:gsub("&", "&amp;"):gsub("<", "&lt;"):gsub(">", "&gt;"))
end

-- Each pptx mark becomes a single flattened raw `<a:r>` run built from the
-- fully-stringified content, since pptx has no per-run home for a marker
-- class alongside pandoc's native Str/Space handling the way docx does.
-- Nested rich formatting inside the marked span/div is lost; this is a
-- documented fallback, not full fidelity.
--
-- `rpr` is the run's `<a:rPr>` element; `label`, if given, maps
-- (el, text) to the displayed text (used for comments, which have no
-- inline-anchored pptx equivalent and so render as a bracketed annotation
-- at the mark's location).
local function pptx_handler(rpr, label)
  return function(el)
    local text = pandoc.utils.stringify(el)
    if label then text = label(el, text) end
    local raw = pandoc.RawInline("openxml",
      "<a:r>" .. rpr .. "<a:t xml:space=\"preserve\">" .. xml_escape(text) .. "</a:t></a:r>")
    if el.t == "Div" then
      return pandoc.Blocks({ pandoc.Para({ raw }) }), false
    end
    return pandoc.Inlines({ raw }), false
  end
end

local pptx_handle_insert = pptx_handler("<a:rPr u=\"sng\"/>")
local pptx_handle_delete = pptx_handler("<a:rPr strike=\"sngStrike\"/>")
local pptx_handle_highlight = pptx_handler(
  "<a:rPr><a:highlight><a:srgbClr val=\"FFFF00\"/></a:highlight></a:rPr>")
local pptx_handle_edit_comment = pptx_handler(
  "<a:rPr i=\"1\"><a:solidFill><a:srgbClr val=\"C00000\"/></a:solidFill></a:rPr>",
  function(el, message)
    return " [" .. (el.attributes["author"] or "comment") .. ": " .. message .. "]"
  end)

--------------------------------------------------------------------------
-- Dispatch
--------------------------------------------------------------------------

local docx_dispatch = {
  ["quarto-insert"] = docx_handle_insert,
  ["quarto-delete"] = docx_handle_delete,
  ["quarto-highlight"] = docx_handle_highlight,
  ["quarto-edit-comment"] = docx_handle_edit_comment,
}

local pptx_dispatch = {
  ["quarto-insert"] = pptx_handle_insert,
  ["quarto-delete"] = pptx_handle_delete,
  ["quarto-highlight"] = pptx_handle_highlight,
  ["quarto-edit-comment"] = pptx_handle_edit_comment,
}

function dispatch_element(el)
  local dispatch = FORMAT == "docx" and docx_dispatch or pptx_dispatch
  local handler = dispatch[first_class(el)]
  if handler then
    return handler(el)
  end
  return nil
end

return {
  {
    traverse = "topdown",
    Span = dispatch_element,
    Div = dispatch_element,
  },
}
