return {
  ['image'] = function(args)
    local path = pandoc.utils.stringify(args[1])
    -- Resolve relative paths against the document directory;
    -- URLs pass through resolve_path unchanged.
    path = quarto.utils.resolve_path(path)
    local mime, content = pandoc.mediabag.fetch(path)
    if content == nil then
      return pandoc.Strong(pandoc.Str("[image fetch failed: " .. path .. "]"))
    end
    local encoded = quarto.base64.encode(content)
    local src = "data:" .. mime .. ";base64," .. encoded
    return pandoc.Image("", src)
  end
}
