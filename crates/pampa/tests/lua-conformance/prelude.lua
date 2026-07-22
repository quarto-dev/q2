-- Environment adapter for the vendored pandoc-lua-marshal test suite.
--
-- Upstream's Haskell driver (test-pandoc-lua-marshal.hs, registerDefault)
-- exposes every constructor as a bare global, the List module as `List`,
-- and every enum constant as a global bound to its own name as a string.
-- This prelude replicates that environment on top of q2's `pandoc` table.
--
-- Run against the production filter environment; see README.md.

for k, v in pairs(pandoc) do
  _G[k] = v
end
List = pandoc.List

-- Enum constants as strings (upstream registerConstants semantics).
-- These overwrite any q2 sentinel values aliased above on purpose:
-- conformance is measured against upstream's environment.
local constants = {
  -- Alignment
  'AlignLeft', 'AlignRight', 'AlignCenter', 'AlignDefault',
  -- ListNumberStyle
  'DefaultStyle', 'Example', 'Decimal',
  'LowerRoman', 'UpperRoman', 'LowerAlpha', 'UpperAlpha',
  -- ListNumberDelim
  'DefaultDelim', 'Period', 'OneParen', 'TwoParens',
  -- MathType
  'DisplayMath', 'InlineMath',
  -- QuoteType
  'SingleQuote', 'DoubleQuote',
  -- CitationMode
  'AuthorInText', 'SuppressAuthor', 'NormalCitation',
}
for _, name in ipairs(constants) do
  _G[name] = name
end
