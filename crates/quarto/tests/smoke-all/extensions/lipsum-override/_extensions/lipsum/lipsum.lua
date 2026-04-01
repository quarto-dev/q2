-- Greek lipsum override: proves user extensions take priority over built-in.
-- Returns Greek placeholder text instead of the standard Latin lorem ipsum.
return {
  ['lipsum'] = function(args, kwargs, meta)
    return pandoc.Para("Λόρεμ ίψουμ δόλορ σιτ αμέτ, κονσεκτετούρ αδιπισίκινγκ ελίτ. Ντούις σαγκίτις ποσουέρε λιγκούλα σιτ αμέτ λακίνια.")
  end
}
