-- book-numbering.lua
-- Copyright (C) 2020-2022 Posit Software, PBC

-- For Typst books, generate a show rule that resets Quarto's custom figure
-- counters at each chapter (level-1 heading). Orange-book only resets
-- kind:image and kind:table, but Quarto uses custom kinds for figures,
-- tables, listings, callouts, custom crossref categories, and math equations.
local function typst_book_counter_reset_rule()
  if not _quarto.format.isTypstOutput() then
    return nil
  end
  if not param("single-file-book", false) then
    return nil
  end

  -- Collect all Typst figure kinds that need counter resets
  local kinds = pandoc.List({})

  for _, category in ipairs(crossref.categories.all) do
    if category.kind == "float" then
      -- Floats use "quarto-float-" .. ref_type (e.g., quarto-float-fig)
      kinds:insert("quarto-float-" .. category.ref_type)
    elseif category.kind == "Block" then
      -- Block kinds (callouts) use "quarto-callout-" .. name (e.g., quarto-callout-Warning)
      -- Only include callout types (they have specific ref_types)
      local callout_ref_types = {nte=true, wrn=true, cau=true, tip=true, imp=true}
      if callout_ref_types[category.ref_type] then
        kinds:insert("quarto-callout-" .. category.name)
      end
    end
  end

  if #kinds == 0 then
    return nil
  end

  -- Build the show rule that resets all counters at chapter boundaries
  local lines = pandoc.List({
    "// Reset Quarto's custom figure counters at each chapter (level-1 heading).",
    "// Orange-book only resets kind:image and kind:table, but Quarto uses custom kinds.",
    "// This list is generated dynamically from crossref.categories.",
    "#show heading.where(level: 1): it => {"
  })
  for _, kind in ipairs(kinds) do
    lines:insert('  counter(figure.where(kind: "' .. kind .. '")).update(0)')
  end
  -- Reset math equation counter at chapter boundaries
  lines:insert('  counter(math.equation).update(0)')
  lines:insert("  it")
  lines:insert("}")

  return table.concat(lines, "\n")
end

function book_numbering()
  return {
    Meta = function(meta)
      -- Inject Typst counter reset show rule into include-before
      local reset_rule = typst_book_counter_reset_rule()
      if reset_rule then
        ensureIncludes(meta, kIncludeBefore)
        meta[kIncludeBefore]:insert(pandoc.Blocks({
          pandoc.RawBlock("typst", reset_rule)
        }))
      end
      return meta
    end,

    -- QUARTO2-PATCH: read the quarto-book-item-* Pandoc attributes the Q2
    -- single-file merge step stamps on every chapter/divider heading,
    -- instead of currentFileMetadataState().file (Q1's own mechanism,
    -- populated by common/filemetadata.lua from the paired
    -- <!-- quarto-file-metadata: ... --> comment markers). This is
    -- additive, not a replacement: the merge step still emits both forms
    -- from the same source data (see claude-notes/plans/
    -- 2026-09-21-book-projects-P2-single-file-merge.md) -- the comment
    -- markers remain the *only* channel the unpatched orange-book Typst
    -- extension's own filter reads via quarto.doc.file_metadata(). This
    -- patch fixes a genuinely dead Q1 read along the way: Q1's
    -- bookItemMetadata (book-render.ts) never sets a `file.appendix`
    -- field, so the "mark appendix chapters for epub" rule below was
    -- unreachable in Q1 too. quarto-book-item-appendix (set only on
    -- appendix chapters, kind Appendix + file Some) is what makes it live.
    Header = function(el)
      local bookItemType = el.attr.attributes["quarto-book-item-type"]
      if bookItemType ~= nil then
        local bookItemDepth = tonumber(el.attr.attributes["quarto-book-item-depth"])
        local bookItemAppendix = el.attr.attributes["quarto-book-item-appendix"] == "true"

        -- if we are in an unnumbered chapter then add unnumbered class
        if bookItemType == "chapter" and el.attr.attributes["quarto-book-item-number"] == nil then
          el.attr.classes:insert('unnumbered')
        end

        -- handle latex "part" and "appendix" headers
        if el.level == 1 and _quarto.format.isLatexOutput() then
          if bookItemType == "part" then
            local partPara = pandoc.Para({
              pandoc.RawInline('latex', '\\part{')
            })
            tappend(partPara.content, el.content)
            partPara.content:insert( pandoc.RawInline('latex', '}'))
            return partPara
          elseif bookItemType == "appendix" then
            local appendixPara = pandoc.Para({
              pandoc.RawInline('latex', '\\cleardoublepage\n\\phantomsection\n\\addcontentsline{toc}{part}{')
            })
            tappend(appendixPara.content, el.content)
            appendixPara.content:insert(pandoc.RawInline('latex', '}\n\\appendix'))
            return appendixPara
          elseif bookItemType == "chapter" and bookItemDepth == 0 then
            quarto_global_state.usingBookmark = true
            local bookmarkReset = pandoc.Div({
              pandoc.RawInline('latex', '\\bookmarksetup{startatroot}\n'),
              el
            })
            return bookmarkReset
          end
        end

        -- Typst part/appendix handling is delegated to book extensions
        -- (each Typst book package has different syntax for parts and appendices)

        -- mark appendix chapters for epub
        if el.level == 1 and _quarto.format.isEpubOutput() then
          if bookItemAppendix and bookItemType == "chapter" then
            el.attr.attributes["epub:type"] = "appendix"
          end
        end

        -- part cover pages have unnumbered headings
        if (bookItemType == "part") then
          el.attr.classes:insert("unnumbered")
        end

        -- return potentially modified heading el
        return el
      end
    end
    -- END QUARTO2-PATCH
  }
end
