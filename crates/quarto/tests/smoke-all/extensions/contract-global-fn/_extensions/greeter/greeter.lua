function greet(args, kwargs, meta)
  return "GREET-" .. pandoc.utils.stringify(args[1])
end
