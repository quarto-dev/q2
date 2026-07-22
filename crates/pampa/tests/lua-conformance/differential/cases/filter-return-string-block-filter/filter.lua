-- Class A1 (oracle probe P2): bare-string return from a Block filter
-- becomes Plain(word-split inlines), like peekBlocksFuzzy.
function Para(p)
  return 'plain text here'
end
