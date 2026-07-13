-- bd-sgfiiktn S1: Citation userdata + Cite(content, citations) arg
-- order. Builds a Cite from typed Citation values, mutates one in
-- place through the citations List (aliased read + :insert persist),
-- and word-splits a string prefix via the fuzzy Inlines peeker.
function Para(p)
  local cit = pandoc.Citation('knuth1984', 'NormalCitation', 'see also', {}, 3, 0)
  local cite = pandoc.Cite('placeholder text', { cit })
  cite.citations[1].mode = 'AuthorInText'
  cite.citations:insert(pandoc.Citation('lamport1994', 'SuppressAuthor'))
  return pandoc.Para({ cite })
end
