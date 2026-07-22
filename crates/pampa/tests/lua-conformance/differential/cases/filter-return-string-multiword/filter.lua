-- Class A1 (oracle probe P1): multi-word bare-string return from an
-- Inline filter is word-split (Str/Space), like peekInlinesFuzzy.
function Str(s)
  if s.text == 'target' then
    return 'two words'
  end
end
