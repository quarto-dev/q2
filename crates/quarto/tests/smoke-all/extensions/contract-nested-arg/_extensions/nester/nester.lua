return {
  outer = function(args)
    return "OUTER[" .. pandoc.utils.stringify(args[1]) .. "]"
  end,
  inner = function()
    return "IN"
  end
}
