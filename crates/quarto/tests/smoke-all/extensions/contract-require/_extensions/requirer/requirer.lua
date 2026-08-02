local helper = require("modules.helper")
return {
  decorated = function(args)
    return "REQ" .. helper.decorate(pandoc.utils.stringify(args[1]))
  end
}
