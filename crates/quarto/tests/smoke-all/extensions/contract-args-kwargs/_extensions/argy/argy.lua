return {
  argy = function(args, kwargs, meta, raw_args, context)
    local a1 = pandoc.utils.stringify(args[1])
    local mode = pandoc.utils.stringify(kwargs['mode'])
    local missing = pandoc.utils.stringify(kwargs['nope'])
    local mtag = (missing == "") and "EMPTY" or "NONEMPTY"
    return "A1=" .. a1 .. ";MODE=" .. mode .. ";MISSING=" .. mtag .. ";NRAW=" .. tostring(#raw_args)
  end
}
