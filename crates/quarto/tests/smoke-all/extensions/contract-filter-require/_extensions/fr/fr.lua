-- GH #587 contract fixture: an extension's Lua *filter* can load sibling
-- modules with require, both by extension-root-relative name and by the
-- absolute form extensions commonly use. Also pins the GH #588 contract on
-- the filter path: resolve_path inside the required module returns the
-- extension-root-resolved path.
local top = quarto.utils.resolve_path("_modules/greet.lua")
local mod = require("_modules/greet")
local abs = require(quarto.utils.resolve_path("_modules/greet.lua"):gsub("%.lua$", ""))

local function tag(ok)
  if ok then
    return "OK"
  end
  return "BAD"
end

return {
  {
    Pandoc = function(doc)
      local line = "fr-require=" .. tag(mod.greeting == "greet-module-loaded")
        .. ";fr-abs=" .. tag(abs.greeting == "greet-module-loaded")
        .. ";fr-resolve=" .. tag(mod.resolved == top)
      doc.blocks:insert(pandoc.Para(pandoc.Str(line)))
      return doc
    end,
  },
}
