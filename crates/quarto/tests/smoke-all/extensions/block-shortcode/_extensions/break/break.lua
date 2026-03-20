return {
  ["break"] = function(args)
    return pandoc.RawBlock("html", '<hr class="ext-break">')
  end
}
