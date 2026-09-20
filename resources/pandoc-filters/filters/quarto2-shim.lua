-- quarto2-shim.lua
-- QUARTO2-PATCH (q2 pandoc-hybrid, P5): the wire-format shim. Decodes Q2's
-- wire-format CustomNode scaffold (a Div/Span carrying the
-- "__quarto_custom_node" class plus "data-custom-*" attributes -- see
-- json.rs:3684-3900) back into real Q1 nodes, so Q1's own render handlers
-- run. Spliced into quarto_filter_list between quarto_init_filters and
-- quarto_normalize_filters -- see resources/pandoc-filters/README.md.
--
-- Design: claude-notes/designs/pandoc-hybrid-architecture.md §3-4.
-- Shape plan: claude-notes/plans/2026-08-20-pandoc-hybrid-P5-lua-shim.md.
--
-- P5 Task 1 shipped the skeleton: the recognizer, the attr sanitizer, the
-- slot collector, and the route table's shape (seven types, "R" or "N", no
-- "L"). Tasks 2-5 replaced every `route_handlers` entry with its real Route
-- R/N reconstruction. Task 6 turned the shared stub (`unwrap_and_drop`)
-- into the unrecognized-type fallback (Path 1) and added the Callout
-- `by_ref_type` guard (Path 2) -- P5's two Q2-only error paths; the third
-- (a Route-R constructor crash) has no Lua-side handling by design, see
-- Task 6's own comments below.

local WIRE_CLASS = "__quarto_custom_node"
local DATA_TYPE_KEY = "data-custom-type"
local DATA_DATA_KEY = "data-custom-data"
local DATA_SLOTS_KEY = "data-custom-slots"
local SLOT_NAME_KEY = "data-slot-name"

-- All wire-format JSON payloads ("data-custom-data", "data-custom-slots")
-- are decoded through `quarto.json` -- Q1's own `_json` module -- and never
-- through `pandoc.json.decode`: pandoc's own JSON decoder represents every
-- number as a Lua float, so an integer `order` (e.g. `1`) comes back as
-- `1.0` and renders as "Figure 1.0" in every numbered document. `quarto.json`
-- preserves the distinction (verified directly against real pandoc; see
-- T1.1).
local function decode_json(str)
  return quarto.json.decode(str)
end

-- Recognizer: true iff `node` is a wire-format custom-node scaffold, i.e. a
-- Div/Span carrying the "__quarto_custom_node" class. Q1's own scaffold
-- (`create_custom_node_scaffold`, `ast/customnodes.lua:236-255`) carries
-- `__quarto_custom*` *attributes* and no classes at all, so the two shapes
-- never collide in either direction.
local function is_wire_node(node)
  return node.classes ~= nil and node.classes:includes(WIRE_CLASS)
end

-- Attr sanitizer: strips the wire-format markers (the "__quarto_custom_node"
-- class and the three "data-custom-*" attribute keys) before `attr` is
-- handed to any Q1 constructor or raw-Div reconstruction, preserving the
-- identifier and every other class/attribute untouched. This is the single
-- place those markers are removed -- an implementation that unwraps a node
-- but re-applies the wrapper's raw attr onto the surviving content would
-- re-arm the exact class-keyed-dispatcher collision the shim's splice
-- position otherwise avoids (see the design doc's Finding on this).
local function sanitize_attr(attr)
  local classes = attr.classes:filter(function(c)
    return c ~= WIRE_CLASS
  end)
  local attributes = {}
  for k, v in pairs(attr.attributes) do
    attributes[k] = v
  end
  attributes[DATA_TYPE_KEY] = nil
  attributes[DATA_SLOTS_KEY] = nil
  attributes[DATA_DATA_KEY] = nil
  return pandoc.Attr(attr.identifier, classes, attributes)
end

-- True iff `el` is the synthetic `Plain` block the writer wraps a
-- block-context Inline/Inlines slot's content in (json.rs:3747-3758).
-- Inline-context (Span) slot children never need this: their content is
-- already shaped as Inlines directly (json.rs's `stream_write_custom_inline`
-- writes Inline slot content straight into the slot Span, no Plain wrapper).
local function is_plain_wrapper(el)
  return el ~= nil and el.t == "Plain"
end

-- Slot collector: pulls each direct `data-slot-name` child out of `node`'s
-- content and unwraps it per `slot_meta`'s declared type
-- ("Block"|"Inline"|"Blocks"|"Inlines" -- json.rs:3690-3701), undoing the
-- `Plain` wrapping block-context Inline/Inlines slots carry.
local function collect_slots(node, slot_meta)
  local slots = {}
  for _, child in ipairs(node.content) do
    local slot_name = child.attributes and child.attributes[SLOT_NAME_KEY]
    if slot_name ~= nil then
      local slot_type = slot_meta[slot_name]
      if slot_type == "Block" then
        slots[slot_name] = child.content[1]
      elseif slot_type == "Blocks" then
        slots[slot_name] = child.content
      elseif slot_type == "Inline" then
        local first = child.content[1]
        if is_plain_wrapper(first) then
          slots[slot_name] = first.content[1]
        else
          slots[slot_name] = first
        end
      elseif slot_type == "Inlines" then
        local first = child.content[1]
        if is_plain_wrapper(first) then
          slots[slot_name] = first.content
        else
          slots[slot_name] = child.content
        end
      end
    end
  end
  return slots
end

-- Decodes a wire-format scaffold into `{type_name, data, slots}`. `data` is
-- the decoded "data-custom-data" payload (nil when the node carries no
-- plain_data at all -- the writer omits the key entirely in that case, see
-- json.rs:3708-3712). `slots` is the name -> content map `collect_slots`
-- produces.
local function decode_wire_node(node)
  local attr = node.attr
  local type_name = attr.attributes[DATA_TYPE_KEY]
  local data_json = attr.attributes[DATA_DATA_KEY]
  local data = nil
  if data_json ~= nil then
    data = decode_json(data_json)
  end
  local slots_json = attr.attributes[DATA_SLOTS_KEY]
  local slot_meta = {}
  if slots_json ~= nil then
    slot_meta = decode_json(slots_json)
  end
  return {
    type_name = type_name,
    data = data,
    slots = collect_slots(node, slot_meta),
  }
end

-- Task 6, Path 1: the unrecognized-type fallback. `dispatch` below now
-- reaches this function only when `wire.type_name` is absent from
-- `route_handlers` -- every one of the seven real types has its own route
-- body since Task 5, so this path fires exclusively for a wire node whose
-- `data-custom-type` names a type this shim build does not know about
-- (a version-skew scenario: Q2 emits a new custom-node type the vendored
-- Q1 filter tree has never heard of). Drop the wrapper, splice the slot
-- content back into the surrounding document unchanged, and emit exactly
-- ONE warning naming the unrecognized type -- not one per slot, which is
-- why the `warn` call sits above the loop rather than inside it. Each slot
-- child's own content is already shaped correctly for its context -- a
-- Div's slot children hold real Blocks (block/blocks slots) or a
-- `Plain`-wrapped Inlines (inline/inlines slots, itself a valid Block),
-- and a Span's slot children hold Inlines directly -- so concatenating
-- every slot child's raw content in order produces a valid Blocks/Inlines
-- splice with no slot-type awareness needed here.
--
-- The wrapper's own attr (including `__quarto_custom_node` and every
-- `data-custom-*` key) is dropped with it, never re-applied to the
-- surviving content: re-applying it would put the wrapper's retained
-- semantic classes (e.g. a stray `.callout-note`) back on a Div that Q1's
-- class-keyed dispatcher (`ast/parse.lua:6-15`) still gets to see,
-- re-arming the exact collision the shim's splice position otherwise
-- avoids -- for the unsupported-type path specifically, since there is no
-- route body left to consume it first.
local function unwrap_and_drop(node, wire)
  warn("quarto2-shim: unrecognized custom node type '" .. tostring(wire.type_name) ..
    "', dropping the wire wrapper and passing its slot content through unchanged")
  local result = pandoc.List({})
  for _, child in ipairs(node.content) do
    for _, item in ipairs(child.content) do
      result:insert(item)
    end
  end
  return result
end

-- Task 2: Route R for Theorem, Proof and FloatRefTarget -- the three
-- fixed-field-set constructors, plus the shared post-construction `order`
-- assignment. `order` is Q2's already-computed crossref order
-- (`crossref_index.rs:287-295`'s `{ section, order }` shape); the shim
-- never calls a Q1 renderer itself -- `main.lua`'s own render pass
-- dispatches once the filter chain returns the converted node.
--
-- `order` always goes onto the constructor's SECOND return value (the data
-- table `custom_node_data` tracks), never the first (the scaffold handed
-- back to the filter chain) -- see design doc §4 / P5 Findings item 2.
-- Nil-tolerant (post-review simplification): every Route-R call site was
-- separately guarding `wire.data.order ~= nil` before calling this; folded
-- into one place instead of three copies that must be kept identical.
local function assign_order(tbl, order_data)
  if order_data ~= nil then
    tbl.order = { order = order_data.order, section = order_data.section }
  end
end

-- Shared "narrow raw-Div reconstruction" fallback: no numbering, no crash,
-- one warning already emitted by the caller. Bypasses the real Q1
-- constructor entirely (rather than constructing it and letting Q1's own
-- render/decoration pass crash later) since nothing downstream needs the
-- real R-type's AST shape once the caller already knows it cannot be
-- built safely. Drops **every** class (not just the wire markers): keeping
-- a semantic class here would re-arm the exact class-keyed-dispatcher
-- collision this shim's splice position otherwise avoids, in
-- `quarto_normalize_filters` -- confirmed the hard way for Callout (Task 6:
-- keeping `callout-note` here reproduced the original crash one filter
-- group later).
local function raw_reconstruction_fallback(attr, title_slot, content_blocks)
  local blocks = pandoc.List({})
  if title_slot ~= nil and #title_slot > 0 then
    blocks:insert(pandoc.Para(title_slot))
  end
  for _, b in ipairs(content_blocks) do
    blocks:insert(b)
  end
  return pandoc.Div(blocks, pandoc.Attr(attr.identifier, {}, attr.attributes))
end

-- Post-review fix: a theorem-classed div whose identifier prefix resolves
-- to a *different*, non-theorem registered ref-type (e.g.
-- `::: {#fig-x .theorem}`) is a case Q2 itself permits, with only a
-- warning (`theorem.rs`'s own sugaring transform: "inconsistent
-- cross-reference specification", proceeds using the class's ref-type and
-- leaves the identifier unchanged). Q1's own renderer, however,
-- re-derives the type from the identifier alone
-- (`customnodes/theorem.lua:221`, `theorem_types[refType(thm.identifier)]`)
-- and crashes on the resulting nil (`theorem_type.env` the next line) --
-- Q1 never anticipated an identifier/class mismatch reaching it, since Q1
-- itself derives the class from the identifier in the first place.
-- Mirrors the Callout `by_ref_type` guard (Task 6) rather than
-- reimplementing `theorem.lua`'s own type resolution.
local function route_theorem(node, wire)
  local attr = sanitize_attr(node.attr)
  if theorem_types[refType(attr.identifier)] == nil then
    warn("quarto2-shim: theorem '" .. attr.identifier ..
      "' has no theorem type registered for its identifier prefix; rendering without theorem styling")
    return raw_reconstruction_fallback(attr, wire.slots.title, wire.slots.content)
  end
  -- Post-review fix: pass a Div carrying the sanitized classes/kv
  -- attributes (not a bare Blocks list) as `div`, so every user class
  -- (`.column-margin`, etc.) and attribute (`data-foo="bar"`, etc.)
  -- survives instead of being dropped into a fresh, empty-attr Div --
  -- `theorem.lua:214-217`'s own Blocks-to-Div normalization only fires
  -- when `div` is a bare Blocks list.
  --
  -- **The identifier on this inner Div must stay blank, though** --
  -- measured, a real second bug found while fixing the first: passing
  -- the REAL identifier (`attr.identifier`, e.g. `"thm-p"`) here
  -- re-triggers `quarto-pre/parseblockreftargets.lua`'s own theorem
  -- detection one filter group later, in `quarto_normalize_filters`.
  -- `is_theorem_div` (`theorem.lua:60-63`, `has_theorem_ref`) matches
  -- purely on the **identifier prefix** resolving via `theorem_types` --
  -- it does not require the `.theorem`-family class at all -- so a
  -- same-identifier inner Div gets wrapped in a SECOND, nested Theorem
  -- emulation, and both get rendered: the numbered caption and the
  -- `"theorem"` class both appeared twice in the captured output before
  -- this was caught (`test_theorem_name_is_user_title` went RED on this
  -- exact fix attempt). The outer `quarto.Theorem{identifier = ...}`
  -- call above already carries the real identifier and
  -- `theorem.lua:216` (`el.identifier = thm.identifier`) restores it
  -- onto this inner Div at render time regardless, so blanking it here
  -- loses nothing.
  local scaffold, tbl = quarto.Theorem{
    identifier = attr.identifier,
    name = wire.slots.title,
    div = pandoc.Div(wire.slots.content, pandoc.Attr("", attr.classes, attr.attributes)),
  }
  assign_order(tbl, wire.data.order)
  return scaffold
end

local function route_proof(node, wire)
  local attr = sanitize_attr(node.attr)
  -- Proof's renderer never reads `order` (proof.lua:81 indexes only
  -- `proof_types[proof_tbl.type:lower()]`), so none is assigned here.
  --
  -- Unlike Theorem's renderer (theorem.lua:211-215), Proof's renderer does
  -- NOT normalize `tbl.div` from Blocks to a Div -- `local el =
  -- proof_tbl.div` (proof.lua:78) is used directly and immediately indexed
  -- as `el.attr.classes` (proof.lua:90). A raw Blocks list has no `.attr`,
  -- so the shim must wrap it here regardless.
  --
  -- Post-review fix: wrap with the sanitized classes/kv attributes, not
  -- a fresh empty attr -- `proof.rs` already strips the `"proof"` class
  -- before the wire write (mirroring Theorem's own stripping), so every
  -- other user class/kv-attribute now survives instead of being
  -- silently dropped. The identifier stays blank on this inner Div for
  -- the same reason as Theorem's (`is_proof_div`, `proof.lua:64-73`,
  -- also has an identifier-prefix-based match branch, keyed off
  -- `crossref.categories.by_ref_type` rather than `theorem_types`) --
  -- `proof.lua:150` (`if type then el.identifier = proof_tbl.identifier
  -- end`) restores it at render time regardless, conditionally on the
  -- identifier resolving to a registered ref-type at all.
  local scaffold = quarto.Proof{
    identifier = attr.identifier,
    name = wire.slots.title,
    div = pandoc.Div(wire.slots.content, pandoc.Attr("", attr.classes, attr.attributes)),
    type = wire.data.type,
  }
  return scaffold
end

local function route_float_ref_target(node, wire)
  local attr = sanitize_attr(node.attr)
  -- `type` is a rename, not a pass-through: Q1's renderer reads
  -- `float.type` as a *display name* keyed into
  -- `crossref.categories.by_name` (common/refs.lua:44-55), which is
  -- exactly Q2's `plain_data.kind` ("Figure"/"Table"/"Listing") -- handing
  -- `plain_data` through verbatim leaves `float.type` nil and crashes
  -- (P5 Findings item 3).
  local scaffold, tbl = quarto.FloatRefTarget{
    attr = attr,
    type = wire.data.kind,
    content = wire.slots.content,
    caption_long = wire.slots.caption_long,
    caption_short = wire.slots.caption_short,
  }
  assign_order(tbl, wire.data.order)
  return scaffold
end

-- Task 3: Route R for Callout and Tabset -- the numbered identity-mapped
-- type and the tabs-list type.
--
-- Callout field map (`callout.lua:79-121`, `slots = { "title", "content" }`
-- at `:77`): identity names throughout. `appearance`/`icon` are already
-- resolved by Q2 before the cut ("Q2 owns presentation defaults",
-- decided 2026-09-17); Q1's own re-normalization (`nameForCalloutStyle`,
-- `callout.lua:18-30`) is idempotent on the values Q2 can produce, so
-- feeding them straight through is safe and no shim-side re-defaulting is
-- needed.
-- Task 6, Path 2: the `by_ref_type` guard. `#thm-x .callout-note`'s prefix
-- ("thm") is a registered *ref type* (`is_valid_ref_type("thm")` is true --
-- theorem types are ref types too) but not a registered *callout/float
-- numbering category* (`crossref.categories.by_ref_type["thm"]` is nil).
-- Q1's own `decorate_callout_title_with_crossref` (called unconditionally
-- from the render pass, `modules/callouts.lua:19-45`) gates on
-- `is_valid_ref_type` FIRST and returns early when it is false -- which is
-- exactly why an ordinary unlabeled callout (`refType("")` is `nil`,
-- `is_valid_ref_type(nil)` is false) or a callout with a non-ref-shaped
-- custom id (`refType("important-note")` is `"important"`,
-- `is_valid_ref_type("important")` is false) never reaches the second
-- gate and must not trigger this fallback. Only when BOTH gates as Q1
-- itself applies them agree (`is_valid_ref_type` true, `by_ref_type` nil)
-- does Q1's later `callout_title_prefix` (`modules/callouts.lua:7-11`)
-- reach its own `fail("unknown callout prefix '" .. ref_type .. "'")` --
-- the shim pre-empts that crash here instead of letting the render abort.
-- Post-review fix: `collapse` is a type/semantics mismatch, not a
-- pass-through. Q1's own `parse` reads `collapse` as the RAW attribute
-- string (`div.attr.attributes["collapse"]`, `nil` when the attribute is
-- absent, `"true"`/`"false"`/etc. otherwise) -- `modules/callouts.lua:
-- 291,295` then reads it as `collapse ~= nil` ("has a collapse toggle at
-- all") and `collapse == "true"` ("starts collapsed"). Q2
-- (`callout.rs:261-263`) already split this single tri-state field into
-- the two separate booleans the constructor needs; feeding
-- `wire.data.collapse` (the "attribute present at all" boolean) straight
-- into Q1's raw-string field would misfire both ways: an ordinary
-- callout with no `collapse=` attribute (`collapse == false`) reads as
-- `collapse ~= nil` (Lua's `false` is not `nil`) and gains a toggle it
-- never asked for; `collapse="false"` (`collapse == true,
-- collapse_starts_collapsed == false`) reads as `collapse == true` and
-- renders **collapsed**, the opposite of the author's intent.
--
-- Extracted as its own named, exposed function (rather than inlined into
-- `route_callout`) because no docx/pptx renderer downstream ever reads
-- the resulting value at all (confirmed: no `collapse` reference
-- anywhere in `quarto-post/docx.lua`) -- this translation cannot be
-- verified via any real render-level test this epic's actual targets
-- support, so it needs to be tested directly as a pure function instead.
local function q1_collapse_value(data)
  if not data.collapse then
    return nil
  end
  return data.collapse_starts_collapsed and "true" or "false"
end

local function route_callout(node, wire)
  local attr = sanitize_attr(node.attr)
  local ref_type = refType(attr.identifier)
  if is_valid_ref_type(ref_type) and crossref.categories.by_ref_type[ref_type] == nil then
    -- Narrow raw-Div reconstruction: no numbering, no crash, one warning.
    -- Bypasses `quarto.Callout` entirely (rather than constructing it and
    -- letting Q1's render pass hit the same `fail()` later) since nothing
    -- downstream needs the real Callout AST shape once we know it cannot
    -- be numbered. Silently dropping the number instead of warning would
    -- re-create exactly what Callout was reclassified L->R to prevent,
    -- plus leave a dangling `@thm-x` reference citing a number that
    -- appears nowhere (design doc §12, final bullet).
    --
    -- **Every class is dropped, not just the wire markers.** Measured:
    -- keeping the sanitized `callout-note` class on the returned Div
    -- re-triggers the exact crash this fallback exists to avoid --
    -- `quarto_normalize_filters` (the very next filter group) contains
    -- Q1's own class-keyed dispatcher, which pattern-matches any Div
    -- carrying a `callout-<style>` class (`customnodes/callout.lua:8`)
    -- and re-parses it into a fresh Callout, which then hits the same
    -- `decorate_callout_title_with_crossref` -> `callout_title_prefix` ->
    -- `fail()` one filter group later. Only the identifier and any
    -- non-class attributes survive; this mirrors Path 1's own discipline
    -- above (drop the wrapper's classes, don't re-arm the dispatcher).
    warn("quarto2-shim: callout '" .. attr.identifier .. "' has ref_type '" .. ref_type ..
      "' with no registered crossref category; rendering without a number")
    return raw_reconstruction_fallback(attr, wire.slots.title, wire.slots.content)
  end

  local scaffold, tbl = quarto.Callout{
    type = wire.data.type,
    appearance = wire.data.appearance,
    icon = wire.data.icon,
    collapse = q1_collapse_value(wire.data),
    title = wire.slots.title,
    content = wire.slots.content,
    attr = attr,
  }
  assign_order(tbl, wire.data.order)
  return scaffold
end

-- Tabset field map (`panel-tabset.lua:147-244`). No `order` -- Tabset is
-- unnumbered. Per-tab slot names are `title-<i>`/`content-<i>`,
-- **zero-based** (`panel_tabset.rs:296-298`'s doc comment says so
-- explicitly, and its `enumerate()` confirms it -- the plan's own prose
-- reads as 1-based but the wire format is not). `plain_data.actives` is a
-- JSON array, which `quarto.json.decode` turns into an ordinary
-- **one-based** Lua table, so the two indices run in parallel but off by
-- one: slot key `"title-" .. i` for `i` in `0..tab_count-1`, but
-- `actives[i + 1]`.
local function route_tabset(node, wire)
  local attr = sanitize_attr(node.attr)
  local tabs = pandoc.List({})
  for i = 0, wire.data.tab_count - 1 do
    tabs:insert(quarto.Tab{
      content = wire.slots["content-" .. i],
      title = wire.slots["title-" .. i],
      active = wire.data.actives[i + 1],
    })
  end
  -- Tabset takes the `need_emulation == false` branch
  -- (`panel-tabset.lua:243`'s `return custom_data, false`), so
  -- `quarto.Tabset{...}` returns `(tbl.__quarto_custom_node, tbl)`
  -- (`ast/customnodes.lua:455-457`) -- the *scaffold* first, the proxy
  -- data table second (P5 Findings item 2: this is the one Route-R
  -- handler that does NOT take the `create_emulated_node` branch other
  -- Route-R types do). Returning `scaffold` (not `tbl`) is what main.lua's
  -- render pass expects to find in the tree.
  local scaffold, tbl = quarto.Tabset{
    level = wire.data.level,
    attr = attr,
    tabs = tabs,
  }
  return scaffold
end

-- Task 4: Route N for CrossrefResolvedRef -- no Q1 constructor exists for
-- this type (Q2 invented it; see design doc §12's accepted asymmetry), so
-- the shim resolves it directly to plain Pandoc inlines by calling Q1's
-- real `resolveRefs`-adjacent globals (`crossref/format.lua`) with Q2's
-- already-resolved `plain_data`, mirroring `resolveRefs`'s `Cite` callback
-- body (`crossref/refs.lua:11-144`) rather than reimplementing its
-- formatting rules. `add_ref_prefix` (`refs.lua:13-19`) is a `local
-- function` declared inside that callback, so it is unreachable from here
-- and is reproduced inline below.
--
-- Deliberate v1 scope-outs (plan Task 4, "Deliberate v1 scope-outs"):
-- subfloat refs (`entry.parent ~= nil`, `refs.lua:104-110`) -- `plain_data`
-- carries no `parent`; multi-ref joining (`refDelim`, `refs.lua:42-45`) --
-- Q2 emits one CrossrefResolvedRef per `@ref`; the `#cite.prefix > 0`
-- branch (`refs.lua:54-55`) -- `cite.prefix` is AST and cannot be a
-- `plain_data` field (P2 Finding 3). All three are simply not reachable
-- from the fields `plain_data` carries, so the branches below cover only
-- what Q2 actually hands the shim.
local function route_crossref_resolved_ref(node, wire)
  local data = wire.data

  -- Post-review fix: `wire.slots.cite_prefix` (`[see @fig-x]`'s "see ")
  -- is now read below -- both branches here originally treated it as
  -- unavailable ("`cite.prefix` is AST and cannot be a `plain_data`
  -- field"), but `crossref_resolve.rs:377-386` deliberately carries it
  -- as a *slot* (Inlines) for exactly this reason, and `collect_slots`
  -- already decoded it into `wire.slots.cite_prefix` -- nothing read it.
  if data.resolved == false then
    -- Mirrors `refs.lua:93-102`'s "not resolve" (global-off) shape, not
    -- the per-ref "no index entry" shape (`refs.lua:127-131`, a `warn` +
    -- `Strong(Str("?@"..label))`) -- the plan takes this one shape for
    -- both cases (Task 4 scope text). `ref-noprefix` mirrors
    -- `#cite.prefix > 0 or cite.mode == pandoc.SuppressAuthor` in full,
    -- now that `cite_prefix` is available.
    local ref_classes = pandoc.List({"quarto-unresolved-ref"})
    local has_cite_prefix = wire.slots.cite_prefix ~= nil and #wire.slots.cite_prefix > 0
    if has_cite_prefix or data.cite_mode == "suppress_author" then
      ref_classes:insert("ref-noprefix")
    end
    return pandoc.Span(stringToInlines(data.identifier), pandoc.Attr("", ref_classes))
  end

  local ref = pandoc.List({})

  -- Inlined `add_ref_prefix` (`refs.lua:13-19`): append `nbspString()`
  -- unless the category opts out or the target is Typst.
  local function add_ref_prefix(prefix)
    ref:extend(prefix)
    local category = crossref.categories.by_ref_type[data.ref_type]
    if (category == nil or category.space_before_numbering ~= false)
      and not _quarto.format.isTypstOutput() then
      ref:extend({nbspString()})
    end
  end

  -- Prefix half: `refs.lua:54-76`. The `[see @fig-x]`-shaped
  -- `cite_prefix` branch takes priority over the category prefix,
  -- exactly mirroring refs.lua's own `elseif` chain -- Q1 REPLACES
  -- "Figure" with the cite's own bracket-prefix text entirely, it does
  -- not combine the two. The `chapters`/"sec" prefix-type juggling
  -- (`refs.lua:60-70`) remains out of scope for the epic's non-chapters
  -- v1 target; `refNumberOption`'s own `type == "sec"` handling below
  -- still applies regardless.
  if wire.slots.cite_prefix ~= nil and #wire.slots.cite_prefix > 0 then
    add_ref_prefix(wire.slots.cite_prefix)
  elseif data.cite_mode ~= "suppress_author" then
    local prefix = refPrefix(data.ref_type, data.label_upper)
    if #prefix > 0 then
      add_ref_prefix(prefix)
    end
  end

  -- Number half: `refs.lua:111-113`, `entry.parent == nil` branch only
  -- (subfloats scoped out above). `refNumberOption` takes an
  -- index-entry-shaped table (`crossref/index.lua:66-78`), not a raw
  -- order -- synthesized here since the shim never looks the label up in
  -- Q1's own `crossref.index.entries` (Q2's `plain_data.order` is already
  -- the resolved value).
  local entry = {
    order = data.order,
    parent = nil,
    caption = pandoc.Blocks({}),
    appendix = false,
  }
  ref:extend(refNumberOption(data.ref_type, entry))

  -- `wire.slots.suffix` (`[@fig-foo, p. 12]`'s ", p. 12") is deliberately
  -- never read: Q1's own `resolveRefs` has no handling for `cite.suffix`
  -- at all (confirmed -- no "suffix" reference anywhere in refs.lua), so
  -- Q1 itself drops a resolved crossref citation's suffix. Per the
  -- Q1-is-normative decision (design doc §12), the shim matches this gap
  -- rather than "fixing" something Q1 doesn't handle either.

  if refHyperlink() then
    return pandoc.Link(ref, "#" .. data.identifier, "", pandoc.Attr("", {"quarto-xref"}))
  else
    return ref
  end
end

-- Task 5: Route N for Equation -- like CrossrefResolvedRef, no Q1
-- constructor exists (Q2 invented this type too); the shim unwraps the
-- wire node back to its `Math` inline and calls Q1's own
-- `renderEquation(eq, label, alt, order)` (`crossref/equations.lua:99-144`)
-- rather than reimplementing the format-specific `RawInline` injection.
-- `alt` is a Typst-only accessibility parameter
-- (`equations.lua:100-101,120-125`), out of scope for docx/pptx and always
-- `nil` here.
--
-- `order` is Q2's already-computed order, but **not always present**
-- (measured correction, post-review: the module comment here previously
-- claimed it was always present, matching P5 Task 5's original design
-- text -- both were wrong). `crossref_index.rs`'s indexer skips writing
-- `plain_data.order` on a duplicate identifier (`index_custom_target`
-- returns early on `self.index.entries.contains_key(&identifier)`,
-- before the order-assignment block), so a second `$$...$$ {#eq-dup}`
-- reaches the shim with `wire.data.order == nil`. Unlike Route R's
-- constructors, `renderEquation`'s non-latex/non-typst branch has no
-- nil-guard on `order` at all (`equations.lua:141-142` unconditionally
-- indexes it) -- Q1 itself never calls it with a nil order, since its own
-- `equations()` filter always computes one synchronously
-- (`indexNextOrder("eq")`, `equations.lua:54`) at the same point it would
-- otherwise skip a duplicate. Degrade to an unnumbered, labelled `Span`
-- instead of crashing, mirroring Q2's own native (non-hybrid) HTML
-- renderer (`crossref_render.rs`'s `.get("order").and_then(...)`) rather
-- than reimplementing `renderEquation`'s numbering logic.
local function route_equation(node, wire)
  local eq = wire.slots.content[1]
  local label = wire.data.identifier
  if wire.data.order == nil then
    return pandoc.Span(eq, pandoc.Attr(label))
  end
  return renderEquation(eq, label, nil, wire.data.order)
end

-- Per-type route bodies. Task 1 wired all seven recognized types to the
-- shared `unwrap_and_drop` stub; Task 2 replaced Theorem/Proof/
-- FloatRefTarget with the real reconstruction above. Task 3 replaces
-- Callout/Tabset. Task 4 replaces CrossrefResolvedRef. Task 5 replaces
-- Equation. Task 6 adds the unrecognized-type warning.
local route_handlers = {
  Callout = route_callout,
  Tabset = route_tabset,
  Theorem = route_theorem,
  Proof = route_proof,
  FloatRefTarget = route_float_ref_target,
  CrossrefResolvedRef = route_crossref_resolved_ref,
  Equation = route_equation,
}

-- Post-review fix: `routes` (the R/N classification `quarto2_shim.routes`
-- exposes for Task 7's schema-totality census) is now DERIVED from
-- `route_handlers` above, rather than a second hand-maintained table --
-- nothing previously enforced that adding a `route_handlers` entry also
-- got a matching `routes` entry (or vice versa); the schema-totality test
-- (T7.3) only ever checked `routes` against the Rust schema, never
-- `route_handlers` against `routes` itself. A type not in `ROUTE_N_TYPES`
-- defaults to `"R"` -- correct for all five current Route-R types, and
-- the right default for any future Route-R addition too (a new Route-N
-- type must be added to `ROUTE_N_TYPES` explicitly, same as it would
-- have needed its own `routes` entry before).
local ROUTE_N_TYPES = {
  CrossrefResolvedRef = true,
  Equation = true,
}
local routes = {}
for type_name, _ in pairs(route_handlers) do
  routes[type_name] = ROUTE_N_TYPES[type_name] and "N" or "R"
end

-- Dispatches a decoded wire node to its route body. An unrecognized
-- `type_name` (absent from `route_handlers`) falls back to
-- `unwrap_and_drop`, which warns exactly once and drops the wrapper (Task
-- 6, Path 1).
local function dispatch(node, wire)
  local handler = route_handlers[wire.type_name] or unwrap_and_drop
  return handler(node, wire)
end

local function convert_if_wire_node(node)
  if not is_wire_node(node) then
    return nil
  end
  local wire = decode_wire_node(node)
  return dispatch(node, wire)
end

-- Exposed for direct testing via `pandoc lua` (T1.1-T1.3) and for Task 7's
-- Layer-1 introspection census, which reads `quarto2_shim.routes` alongside
-- Q1's own handler registry.
quarto2_shim = {
  decode_wire_node = decode_wire_node,
  sanitize_attr = sanitize_attr,
  routes = routes,
  q1_collapse_value = q1_collapse_value,
}

quarto_pandoc_shim_filters = {
  { name = "quarto2-wire-shim",
    filter = {
      Div = convert_if_wire_node,
      Span = convert_if_wire_node,
    },
    traverser = 'jog',    -- the walker, matching every main.lua entry
    -- No top-down traversal flag on the filter table: the contract is
    -- BOTTOM-UP, so a nested wire node (a FloatRefTarget inside a Callout's
    -- content slot) is converted inner-first and the outer constructor
    -- receives a real Q1 scaffold. The traversal-direction flag is a
    -- property of the *filter* table (ast/runemulation.lua:132 is the only
    -- top-down user in the tree); `traverser` above selects the *walker*
    -- (ast/customnodes.lua:76-88) and is a separate concern.
  },
  -- H7 (P5 Task 7): the Layer-1 registry census. Inert unless
  -- QUARTO2_LAYER1_DUMP names a path -- appended LAST in this group so it
  -- observes Q1's live `by_ast_name` handler registry (populated by every
  -- `_quarto.ast.add_handler` call inside `quarto_init_filters`, which
  -- P4 Task 8's splice already guarantees ran before this group) and the
  -- shim's own route table above, both fully constructed. Returns `nil`
  -- (not `doc`) on every path, so it can never perturb the AST the Layer-2
  -- goldens (Task 8) assert on -- T7.7 binds this.
  { name = "quarto2-layer1-census",
    traverser = 'jog',
    filter = {
      Pandoc = function(doc)
        local out = os.getenv("QUARTO2_LAYER1_DUMP")
        if out == nil then return nil end          -- inert: no AST change, no cost
        local census = {}
        for ast_name, h in pairs(
          quarto_global_state.extended_ast_handlers.handlers.by_ast_name
        ) do
          census[ast_name] = {
            kind       = h.kind,
            class_name = h.class_name,
            slots      = h.slots,                  -- nil stays nil; see T7.6
          }
        end
        -- The Route-N arity probe (T7.4): every global the shim's own
        -- Route-N bodies call (`refPrefix`, etc., Task 4) must exist and
        -- accept the recorded parameter count. These are Q1 globals
        -- (`crossref/format.lua`, `crossref/options.lua`, `common/pandoc.lua`),
        -- reachable here only because this filter runs inside `main.lua`'s
        -- own Lua state after every import has executed -- unlike
        -- T1.1-T1.3's standalone `pandoc lua` probe, which never loads the
        -- vendored share tree at all.
        local arity_names = {
          "refPrefix", "refNumberOption", "subrefNumber", "refHyperlink",
          "refDelim", "crossrefOption", "nbspString",
        }
        local arity = {}
        for _, name in ipairs(arity_names) do
          local fn = _G[name]
          arity[name] = {
            is_function = type(fn) == "function",
            nparams = type(fn) == "function" and debug.getinfo(fn, "u").nparams or -1,
          }
        end
        local f = io.open(out, "w")
        if f then                                  -- warn, never abort: see below
          f:write(quarto.json.encode({
            handlers = census,
            routes   = quarto2_shim.routes,         -- the shim-facing direction, T7.3
            arity    = arity,
          }))
          f:close()
        else
          -- Ships in the production shim; a mistyped QUARTO2_LAYER1_DUMP
          -- must not be able to fail a user's render (Q1's own idiom,
          -- `crossref/index.lua:131-138`).
          warn("quarto2-layer1-census: cannot write " .. out)
        end
        return nil                                 -- document passes through unchanged
      end,
    },
  },
}
