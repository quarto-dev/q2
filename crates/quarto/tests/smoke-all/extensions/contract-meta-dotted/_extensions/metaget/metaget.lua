return {
  getmeta = function(args, kwargs, meta)
    local key = pandoc.utils.stringify(args[1])
    local v = meta[key]
    if v == nil then return "META-NIL" end
    return "META[" .. pandoc.utils.stringify(v) .. "]"
  end,
  chainmeta = function(args, kwargs, meta)
    local v = meta.custom.nested.value
    if v == nil then return "CHAIN-NIL" end
    return "CHAIN[" .. tostring(v) .. "]"
  end,
  boolmeta = function(args, kwargs, meta)
    local v = meta.flag
    return "BOOL[" .. tostring(v) .. "]"
  end
}
