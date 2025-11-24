# Doctemplates Nesting Directive (`$^$`) Analysis

**Date**: 2025-11-24
**Related Issue**: k-379 - Port Pandoc template functionality
**Source**: Analysis of jgm/doctemplates Haskell source code

## Overview

The `$^$` directive controls **indentation of multi-line content**. When a variable's value contains multiple lines, `$^$` tells the renderer to indent all continuation lines so they align with the column where `$^$` appears.

## The Key Question: When Does `$^$` Stop?

**The effect of `$^$` is determined by column position, not by an explicit end delimiter.**

This is similar to Python's indentation-sensitive syntax - the indentation itself defines the scope.

## Source Code Analysis

### Parser State

The parser maintains a `nestedCol :: Maybe Int` field in its state (`Parser.hs` line 58):

```haskell
data PState =
  PState { templatePath    :: FilePath
         , partialNesting  :: !Int
         , breakingSpaces  :: !Bool
         , firstNonspace   :: P.SourcePos
         , nestedCol       :: Maybe Int      -- <-- Nesting column
         , insideDirective :: Bool
         }
```

### Parsing `$^$` (`pNested` function, lines 214-227)

```haskell
pNested :: (TemplateTarget a, TemplateMonad m) => Parser m (Template a)
pNested = do
  col <- P.sourceColumn <$> P.getPosition   -- 1. Record column of $^$
  pEnclosed $ P.char '^'
  oldNested <- nestedCol <$> P.getState
  P.updateState $ \st -> st{ nestedCol = Just col }  -- 2. Set nesting column
  x <- pTemplate                                      -- 3. Parse following content
  xs <- P.many $ P.try $ do
          y <- mconcat <$> P.many1 pBlankLine
          z <- pTemplate
          return (y <> z)
  let contents = x <> mconcat xs
  P.updateState $ \st -> st{ nestedCol = oldNested }  -- 4. Restore old nesting
  return $ Nested contents
```

### Line Continuation Logic (`pEndline` function, lines 71-84)

This is where the "bracketing" happens:

```haskell
pEndline :: Monad m => Parser m String
pEndline = P.try $ do
  nls <- pLineEnding
  mbNested <- nestedCol <$> P.getState
  inside <- insideDirective <$> P.getState
  case mbNested of
    Just col -> do
      -- Skip whitespace up to (but not past) the nesting column
      P.skipMany $ do
        P.getPosition >>= guard . (< col) . P.sourceColumn
        P.char ' ' <|> P.char '\t'
      curcol <- P.sourceColumn <$> P.getPosition
      -- CRITICAL: Line must be at or past the nesting column
      guard $ inside || curcol >= col
    Nothing  ->  return ()
  return nls
```

### AST Representation (`Internal.hs` line 68)

```haskell
data Template a =
       Interpolate Variable
     | Conditional Variable (Template a) (Template a)
     | Iterate Variable (Template a) (Template a)
     | Nested (Template a)    -- <-- Wraps nested content
     | Partial [Pipe] (Template a)
     | Literal (Doc a)
     | Concat (Template a) (Template a)
     | Empty
```

### Rendering (`Internal.hs` lines 447-449)

```haskell
renderTemp (Nested t) ctx = do
  n <- S.get                    -- Current column from state
  DL.nest n <$> renderTemp t ctx  -- Apply DocLayout's nest function
```

The `DL.nest n` function from the `doclayout` library indents all lines of the content by `n` spaces.

## Bracketing Rules

### Start
When `$^$` is encountered, the parser records its **column position** as the "nesting column".

### Continuation
After a newline, the parser:
1. Skips whitespace up to (but not past) the nesting column
2. Checks if the current column is at or past the nesting column
3. If yes, the line is included in the nested content
4. If no (and not inside a directive), `pEndline` fails, ending the nesting

### End
The nesting effect ends when:
- A line starts at a column **before** the nesting column (causes `pEndline` to fail)
- The parser naturally reaches the end of the template
- A blank line is followed by content at a lower column

## Example Walkthrough

### Template:
```
$item.number$  $^$$item.description$ ($item.price$)
               (Available til $item.sellby$.)
```

### Column Analysis:
```
         1111111111222222222233333333334
1234567890123456789012345678901234567890
$item.number$  $^$$item.description$ ($item.price$)
               (Available til $item.sellby$.)
               ^
               Column 16: nesting column
```

### Parsing Behavior:
1. Parser sees `$^$` at column 16, sets `nestedCol = Just 16`
2. Parses `$item.description$ ($item.price$)` as nested content
3. Hits newline, `pEndline` runs
4. Skips whitespace on line 2 up to column 16
5. Sees `(Available` at column 16 (>= 16), so line 2 is included
6. If line 3 started at column 1, it would **not** be included

### Output (with multi-line description):
```
00123  A fine bottle of 18-year old
       Oban whiskey. ($148)
       (Available til March 30, 2020.)
```

## Summary Table

| Aspect | How it works |
|--------|--------------|
| **Start** | `$^$` marks the nesting column |
| **End** | Implicitly when subsequent lines start left of the nesting column |
| **Scope** | All content on same line + continuation lines aligned at/past the column |
| **Effect** | Multi-line values get their continuation lines indented to the nesting column |
| **AST** | `Nested (Template a)` wraps the content |
| **Render** | `DL.nest n` applies indentation |

## Implications for Tree-Sitter Grammar

Since tree-sitter doesn't have parser state in the same way Parsec does, we have several options:

1. **Simple approach**: Just parse `$^$` as a token and handle nesting semantics in the evaluator
2. **Scanner approach**: Use an external scanner to track column positions and emit nesting end tokens
3. **Semantic approach**: Parse the structure loosely and validate/interpret column semantics post-parse

For our initial implementation, option 1 (simple approach) is recommended. The column-based bracketing is fundamentally a semantic concern that the template evaluator can handle.

## Related Files

- `jgm/doctemplates/src/Text/DocTemplates/Parser.hs` - Parser implementation
- `jgm/doctemplates/src/Text/DocTemplates/Internal.hs` - AST and renderer
- `jgm/doctemplates/README.md` - User documentation
