-- GH #588 contract fixture: quarto.utils.resolve_path must return the same
-- extension-root-resolved path from the top-level script, from a file loaded
-- with require, and from a file loaded with dofile.
local top = quarto.utils.resolve_path("_modules/greet.lua")
local by_require = require("_modules/probe").resolved
local by_dofile = dofile(quarto.utils.resolve_path("_modules/probe.lua")).resolved

local function tag(p)
  if p == top then
    return "SAME"
  end
  return "DIFF"
end

return {
  rp = function()
    return "rp-top=OK;rp-require=" .. tag(by_require) .. ";rp-dofile=" .. tag(by_dofile)
  end,
}
