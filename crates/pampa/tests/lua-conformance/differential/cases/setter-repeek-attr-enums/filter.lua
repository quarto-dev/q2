-- bd-0g2yp61w: property assignment re-runs the constructor peekers.
-- attr accepts every Pandoc shape on assignment (bare string,
-- positional triple, HTML-like map); quotetype/mathtype setters work.
function Header(h)
  h.attr = 'assigned-id'                 -- bare string -> identifier
  return h
end

function Span(s)
  s.attr = { '', {}, { { 'a', 'b' } } }  -- positional triple
  return s
end

function Code(c)
  c.attr = { id = 't', fubar = 'quux' }  -- HTML-like map
  return c
end

function Quoted(q)
  q.quotetype = 'DoubleQuote'
  return q
end

function Math(m)
  m.mathtype = 'DisplayMath'
  return m
end
