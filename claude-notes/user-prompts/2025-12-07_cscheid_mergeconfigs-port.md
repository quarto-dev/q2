I want to work on redesigning/porting the YAML configuration system that Quarto uses. In the typescript version of quarto-cli, this system is based around the mergeConfigs() function. (You have access to the quarto-cli source code in external-sources/quarto-cli)

In the past, we've studied how it works inside quarto-cli, in the following plan files:

- claude-notes/config-merging-analysis.md
- claude-notes/session-logs/session-log-2025-10-11.md

I've then done some additional work on a separate repository studying a model Haskell implementation of the problem, in /Users/cscheid/repos/cscheid/composable-validation. That repository has notes you wrote in /Users/cscheid/repos/cscheid/composable-validation/claude-notes/*.md

The main observation in the composable-validation directory is that we can reproduce all of the (complex, but necessary) behavior of quarto-cli's mergeConfigs and related functions with a system that performs different merging operations depending on the _tags_ of the values associated with the YAML object: "!prefer" and "!concat". The composable-validation repository has a fuller explanation. (That repository was also concerned with the behavior of the validator library under this merging operation. We'll want to worry about that as well, but not in this current session)

Here, I want to design the rust library equivalent of mergeConfigs so that code in pampa can use it to determine the configuration of quarto-markdown objects for execution.

The "configuration" of a quarto-markdown document will be defined as the result of merging some "project-level" metadata with all the YAML metadata blocks in the document, producing a final result that will either be interpreted by pampa itself during (eg) citeproc or template rendering, or be usable by filters or other parts of the Rust code base.

## semantics

Our code will need the ability to "interpret" a sequence of configuration objects as a single object of its own, from which values can be retrieved. 

### example

The project-wide metadata object (`_quarto.yml` or some such) might be

```yaml
author: 
  - John Doe
  - Carlos Scheidegger
```

And a document has:

```markdown
---
title: Hello world
author:
  - Jack Sparrow
---

This is a document.
```

Then, as far as the document is concerned, its configuration has _three_ values in the `author` field: `John Doe`, `Carlos Scheidegger`, and `Jack Sparrow`. If, instead, the document had

```
author: !prefer
  - Jack Sparrow
```

Then the author field would be a single-value array `Jack Sparrow`. The `composable-validation` crate has the rules for how to decide between concatenating arrays vs choosing values.

### multiple "top-level" document metadata blocks

Pandoc interprets metadata blocks to always be in the top level. pampa has the notion of "block-level metadata", designed to eventually allow filters to be "scoped", where the structure of the AST determines the scope. Still, pampa should merge all metadata blocks that exist before the first "markdown block" of a document, and consider that to be the overall configuration for the document.

This will (intentionally) cause diverging behavior between Pandoc and pampa. In Pandoc, there's no value merging of objects. Given two blocks with array values, the latter "wins" (that is, it's implicitly `!prefer` behavior everywhere). We will have implicit rules about values merging with particular semantics, and explicit `!prefer` and `!concat` choices.

The reason I want multiple top-level document metadata blocks is that I want the semantics of a document configuration to be, straightforwardly, adding the project-level YAML configuration to the Pandoc AST (likely as a Rust filter that prepends the project configuration). The result is that the project configuration should behave as "default" values for the document, but allow the .qmd file to "override" all values as desired.

### Markdown values vs YAML values

Another complication is that metadata values in .qmd documents have markdown semantics associated with it:

```md
---
title: This is **strong**, and [this is a link](https://example.com)
---
```

In _quarto.yml, these values are represented as strings by default. merge_configs will choose
concat vs prefer depending on context, and also allow explicit semantics. Similarly, I think that our configuration system should have default behaviors depending on context, and also allow explicitly determining value interpretation rules.

This already exists in the .qmd files: values in the front matter can have `!md` and `!str` to control their interpretation. We should allow these tags (and potentially others) to be used in regular quarto-yaml objects, so that when a "plain" quarto-yaml configuration is added to a .qmd document, we have the ability to interpret those values as either markdown or other values.

In terms of implementation, this might be slightly complicated and maybe will need refactoring - I believe quarto-yaml and pampa use different top-level structs; we'll need something like:

- the ability to convert between the two
- a unification of the two structs
- a third type that behaves like "the union" of the two, and then merge_config can be implemented over that type.

I don't know which is the best choice.

### Constraint: source location preservation

We want the ability to do validation over the merged objects, so whatever implementation we end up doing will need
to be able to resolve the source location of merged values to the source location indicated by the original objects that existed before the merge

### Performance, API design

There are at least two possible choices for the overall API design:

#### "eager" evaluation: this is how quarto-cli works

We have a function merge_configs that takes an array of configuration objects and returns a new configuration object, from which we can look up values as necessary. This is straightforward to implement, but incurs a performance penalty per invocation of the merging function, independently of whether the objects themselves are referenced.

In Rust, this would also involve a large amount of clone() calls to return new values.

#### "lazy" or "deferred" evaluation

We can create a trait for configuration objects and have multiple implementations of this trait.

This is a more complex implementation, but would be beneficial in the sense that a vector of configuration objects
would not have to be "eagerly cloned": a deferred evaluator could maintain a vector of reference to other configuration objects, and then offer resolution at the "navigation site": when something in the code base requests the value of the merged object.

This would require a more complicated management of lifetimes, especially if subobjects are complex. Consider the following case:

```yaml
# obj1
outer:
  inner:
    foo: bar

# obj2
outer:
  inner:
    baz: bah
```

According to the semantics of merge_configs (and using the syntax of composable-validation), `obj1 <> obj2` should
behave like the object

```yaml
foo: bar
baz: bah
```

But this object itself holds references to subobjects of obj1 and obj2, and so the lifetimes would have to be carefully managed. To be clear, this seems possible: the subobject would have a shorter lifetime than `obj1 <> obj2`, which itself would have a shorter lifetime than `obj1` and `obj2` both.

In addition, we could have a "clone" trait implementation that performs the eager resolution and returns a full new value with its own lifetime.

The main reason I want to spend a while designing this carefully is that we have evidence that, in the quarto-cli, mergeConfigs is responsible for about 15% of the _total_ runtime of the application.

## Plan for this session

I've now presented with all the existing constraints and goals for the design. Now, I want you to read the source code of quarto-yaml, quarto-source-info, pampa, and /Users/cscheid/repos/cscheid/composable-validation, and I want you to propose a plan for how to design a configuration system for the new port.

This is purely a design/planning session, and we are not going to be doing any implementation work. As a result, I want you to take your time considering alternatives, tradeoffs, and upsides/downsides. Please crate a beads issue to track your work, and write your analysis/plan into a document linked to the issue. Take your time, ask me clarifying questions as needed, and ultrathink.