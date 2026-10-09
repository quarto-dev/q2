# caret selection and structural editing on a rendered document

In this note we will not concern ourselves with the UX/UI implementation, that has been explored in the `experiment/advanced-careting` branch and we will bring a simplified approach informed by that in later.

html rendered document <-> ast <-> source

The goal of this is to come up with a good model and structure for approaching problems of editing ASTs in a text-like way.

## Diffs, Indices, Index mapping
Note that we need to produce minimal diffs both ways.

source edit -> minimal ast edit

ast edit -> minimal source edit

To achieve this, and other things, we will want a mapping between

source string indices. Like:
```
0h1e2l3l4o5
````
and
ast indices
```
0 
para{
  1
  text{2h3e4l5l6o7}
  8
}
9
```
note how indices appear "in-between" nodes in the AST (and in the string). In other words, every node has a before index and an after index. The after index of a node
and the before index of the next node are identified.

a parse (with tie breaking) can induce a surjective function from string indices to ast indices.

In the above example, the induced function would be
```
0 -> 0, 1, 2
1 -> 3
2 -> 4
3 -> 5
5 -> 6
5 -> 7, 8, 9.
```
We will want implementations of functions in both directions for doing this mapping.
iAstFromiStr: Number -> Number[]
and iStrFromiAst: Number -> Number

## Valid ASTs
Now, parses often throw away information. for example: extra whitespace; but
we will want to deal with ASTs that have extra whitespace because
it'll feel natural to type that when structurally editing an AST. That's fine as long as we have a way to write that AST back to a string. Let us consider parsers that throw away information to be sub-surj-parsers of sup-isoparsers that do not throw away info. For our theory for now, we consider the domain of the sup-isoparser to be the set of valid ASTs, rather than the domain of the sub-surj-parser.

A parser always gives a reverse: a writer. We need a way to construct a writer from a parser, or construct a parser and writer at the same time, both from a grammar.

## AST Cursor
The AST cursor can move to any index in the AST. For the purposes of cursoring, we will sometimes want to identify multiple indices in the AST as one I think. I don't quite have a theory for that yet. In the meantime we should try to maintain granular control over it.
Carets move in response to insertions and deletions.

## Stable identity
If the AST changes elsewhere, we need to be able to maintain cursor positions. This means cursors are not just pointers to indices, they are pointers to stable positions in the AST (they give an index at any point in the AST's history tho). Tho maybe we don't need the concept of indices if we have a concept of stable position? How can we do this?

## AST Caret
An AST caret is an AST cursor that has another cursor paired with it to represent a selection. The other cursor in the pair is called the anchor. Shift+move drops the anchor and moves the caret. For user facing things, a caret always has an anchor paired with it, and their selection is empty while they overlap.

## AST editing
Pressing backspace at an index in the AST should delete the before node. Typing should insert a character after the index. This may depend where we are in the AST tho (e.g. space in a latex matrix should add a `&`, enter should add a `\\`; sometimes we may want to join nodes instead of deleting the previous, idk). We should try to maintain granular control over this (we may want to be able to define it locally, per adjacent node-types). If there is a selection, all nodes contained completely within the selection should be deleted.

After editing the AST, for each local edit, we should be able to know which part of the AST is edited, write those to strings, and using knowledge of where those parts of the AST came from in the source string do a minimal splice on the source string to update it.

## Considerations and Questions

- For the most part for AST editing, we don't actually want to consider the induced index function. We only need that for doing diffs.

- we will want structural copy&paste
  - we will find the closest common ancestor of all the nodes in the selection and grab the subtree from that node to the selected nodes. This may not work in all cases but I believe its the most neutral starting choice.
- right to left text would complicate this, let's briefly consider it then ignore it for now.
- depending on node type, we will not always want text-like selection in our AST UI. For example, if part of the AST represents a graphic, we may want rectangular selection which would correspond to a non-contiguous span of source. Thats okay, we will come back to that.