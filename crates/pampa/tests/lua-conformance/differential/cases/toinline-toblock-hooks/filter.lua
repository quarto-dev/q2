-- bd-olz91r4v: __toinline/__toblock metamethod hooks. Objects with
-- the metamethods coerce wherever the fuzzy peekers run (constructor
-- args AND filter returns, both element and singleton-list
-- positions); a non-function metafield falls through to normal list
-- interpretation.
local my_code = setmetatable(
  { code = 'open access', id = 'opn' },
  { __toinline = function(t)
      return pandoc.Code(t.code, { id = t.id })
    end }
)
local my_block = setmetatable(
  { text = 'raised' },
  { __toblock = function(t)
      return pandoc.CodeBlock(t.text)
    end }
)
local ignored = setmetatable({ 'plain' }, { __toinline = true })

function Para(p)
  return {
    pandoc.Para(pandoc.Inlines(my_code)),   -- singleton via hook
    pandoc.Para(pandoc.Inlines(ignored)),   -- non-function -> list
    pandoc.Div(my_block),                   -- __toblock in Blocks position
  }
end
