-- Class A1, topdown traversal: exercises the *_with_control return
-- handlers with a bare-string return.
traverse = 'topdown'
function Str(s)
  if s.text == 'target' then
    return 'replaced'
  end
end
