return {
  shorty = function(args)
    if args[1] == "error" then
      return quarto.shortcode.error_output("shorty", "error message", "inline")
    else
      return pandoc.Strong(args[1])
    end
  end
}
