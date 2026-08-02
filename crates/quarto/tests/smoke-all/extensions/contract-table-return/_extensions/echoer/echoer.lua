return {
  ['dash-name'] = function(args)
    return "DASH-OK"
  end,
  ctx = function(args, kwargs, meta, raw_args, context)
    return "CTX-" .. context
  end
}
