-- resourceRefs.lua
-- Copyright (C) 2020-2022 Posit Software, PBC

local function handle_raw_element_resource_ref(el)
  if _quarto.format.isRawHtml(el) then
    local file = currentFileMetadataState().file
    if file ~= nil and file.resourceDir ~= nil then
      handlePaths(el, file.resourceDir, resourceRef)
      return el
    end
  end
end

function resourceRefs() 
  
  return {
    Image = function(el)
      -- In a single-file book the Rust merge has already re-anchored
      -- chapter-relative image targets (project/book/images.rs); doing it
      -- again would prepend the chapter directory twice.
      if param("single-file-book", false) then
        return el
      end
      local file = currentFileMetadataState().file
      if file ~= nil and file.resourceDir ~= nil then
        el.src = resourceRef(el.src, file.resourceDir)
      end
      return el
    end,

    RawInline = handle_raw_element_resource_ref,
    RawBlock = handle_raw_element_resource_ref,
  }
end
