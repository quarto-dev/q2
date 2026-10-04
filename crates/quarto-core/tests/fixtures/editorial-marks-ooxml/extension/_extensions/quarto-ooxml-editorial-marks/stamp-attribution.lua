-- stamp-attribution.lua
--
-- Runs at pre-quarto, inside q2's own Lua engine (the only place the
-- `quarto.attribution` API and node source ranges exist; the docx filter
-- runs later in a pandoc subprocess where neither is available). Copies the
-- blamed author and time of each editorial mark onto the mark as `author=`
-- and `date=` attributes, which ooxml-editorial-marks.lua turns into
-- w:author / w:date. Attributes written explicitly in the source win.
--
-- Does nothing when attribution is off or a mark's source range does not
-- resolve to a blamed run: lookup() then returns nil and the mark is left
-- unchanged.

local MARK_CLASSES = {
  ["quarto-insert"] = true,
  ["quarto-delete"] = true,
  ["quarto-edit-comment"] = true,
}

-- Providers differ in the unit of `time` (git blame gives seconds). Anything
-- above 1e11 would be past the year 5000 as seconds, so is taken as ms.
local function epoch_seconds(t)
  if t > 1e11 then
    return t // 1000
  end
  return t
end

local function stamp(el)
  if not MARK_CLASSES[el.classes[1]] then
    return nil
  end
  local hit = quarto.attribution.lookup(el)
  if not hit then
    return nil
  end
  if el.attributes["author"] == nil then
    el.attributes["author"] = hit.name
  end
  if el.attributes["date"] == nil then
    el.attributes["date"] = os.date("!%Y-%m-%dT%H:%M:%SZ", epoch_seconds(hit.time))
  end
  return el
end

return {
  { Span = stamp, Div = stamp },
}
