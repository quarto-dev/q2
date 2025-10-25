/**
 * Experimental file to explore recursive type annotation strategies
 *
 * Simplified AST with only two node types:
 * - Str: leaf node containing text
 * - Span: container node with classes and child inlines
 */

// ============================================================================
// Base (non-annotated) AST types
// ============================================================================

export type Inline_Str = {
  t: "Str";
  c: string;
};

export type Inline_Span = {
  t: "Span";
  classes: string[];
  c: Inline[];
};

export type Inline = Inline_Str | Inline_Span;

// ============================================================================
// Attempt 1: Simple intersection (WRONG - doesn't recurse)
// ============================================================================

export type Annotated_Inline_Str_Wrong = Inline_Str & { s: number };

export type Annotated_Inline_Span_Wrong = Inline_Span & { s: number };
// Problem: This gives us { t: "Span"; classes: string[]; c: Inline[]; s: number }
//          But c: Inline[] should be c: Annotated_Inline[]!

export type Annotated_Inline_Wrong = Annotated_Inline_Str_Wrong | Annotated_Inline_Span_Wrong;

// ============================================================================
// Examples demonstrating the problem
// ============================================================================

// This compiles but is wrong!
const wrongExample: Annotated_Inline_Span_Wrong = {
  t: "Span",
  classes: ["emphasis"],
  c: [{ t: "Str", c: "hello" }], // <-- Non-annotated Inline! Should require 's' field
  s: 42
};

// TypeScript doesn't catch this because Inline_Span & { s: number } doesn't
// transform the nested Inline[] reference

// ============================================================================
// Attempt 2: Parameterized base types (exploring recursive construction)
// ============================================================================

export type Base_Span<T> = {
  t: "Span";
  classes: string[];
  c: T[];
};

// Question: Can we define Inline_Span recursively through itself?
// export type Inline_Span_V2 = Base_Span<Inline_Span_V2>;
// RESULT: NO - TypeScript error TS2456: Type alias circularly references itself

// For annotated version:
export type Annotated_Base_Span<T> = Base_Span<T> & { s: number };

// export type Annotated_Span_V2 = Annotated_Base_Span<Annotated_Span_V2>;
// RESULT: NO - same circular reference error

// ============================================================================
// Analysis: Why doesn't this work?
// ============================================================================

// If Inline_Span_V2 = Base_Span<Inline_Span_V2> were allowed, we'd get:
// Inline_Span_V2 = { t: "Span"; classes: string[]; c: Inline_Span_V2[] }
//
// PROBLEM: This creates a Span that can ONLY contain other Spans!
// We've lost the ability to have Str children.
//
// Even if TypeScript allowed it, the semantics would be wrong for our AST.

// ============================================================================
// Attempt 3: Using interfaces for recursive types
// ============================================================================

// TypeScript errors: type aliases cannot circularly reference themselves!
// But interfaces CAN be recursive. Let's try using interfaces.

export interface Inline_V3 {
  t: "Str" | "Span";
  c: any; // We'll narrow this
}

// Hmm, but we lose the discriminated union benefits...
// Let me try a different approach: interfaces that extend types

export type Inline_Str_V3 = {
  t: "Str";
  c: string;
};

export type Base_Span_V3<T> = {
  t: "Span";
  classes: string[];
  c: T[];
};

// Now define the union as an interface that can be one of these
// Actually, this won't work well either...

// ============================================================================
// Attempt 4: Two-phase approach - define structure, then add recursion
// ============================================================================

// What if we define the individual node structures first (non-recursive),
// then create recursive versions by explicit union construction?

// Phase 1: Define non-recursive "shapes"
type Str_Shape = {
  t: "Str";
  c: string;
};

type Span_Shape<TChildren> = {
  t: "Span";
  classes: string[];
  c: TChildren[];
};

// Phase 2: Manually construct the recursive union
// export type Inline_V4 = Str_Shape | Span_Shape<Inline_V4>;
// RESULT: NO - TypeScript error TS2456: Type alias circularly references itself

// export type Annotated_Inline_V4 =
//   | Annotated_Str_Shape
//   | Annotated_Span_Shape<Annotated_Inline_V4>;
// RESULT: NO - same error

// ============================================================================
// Attempt 5: What DOES work in TypeScript?
// ============================================================================

// TypeScript DOES allow recursion when it's "behind" an object property:
type TreeNode = {
  value: number;
  children: TreeNode[];  // This works!
};

// Can we use this pattern? Let's try wrapping everything in objects:

interface Inline_V5_Str {
  t: "Str";
  c: string;
}

interface Inline_V5_Span {
  t: "Span";
  classes: string[];
  c: Inline_V5[];  // Recursive reference to the union
}

type Inline_V5 = Inline_V5_Str | Inline_V5_Span;

// SUCCESS! This works because the recursion is behind the 'c' property.

// Now can we do the same for annotated?
interface Annotated_Inline_V5_Str {
  t: "Str";
  c: string;
  s: number;
}

interface Annotated_Inline_V5_Span {
  t: "Span";
  classes: string[];
  c: Annotated_Inline_V5[];  // Recursive reference to annotated union
  s: number;
}

type Annotated_Inline_V5 = Annotated_Inline_V5_Str | Annotated_Inline_V5_Span;

// Test it:
const testV5: Annotated_Inline_V5_Span = {
  t: "Span",
  classes: ["test"],
  c: [
    { t: "Str", c: "hello", s: 5 },
    {
      t: "Span",
      classes: ["nested"],
      c: [{ t: "Str", c: "world", s: 10 }],
      s: 15
    }
  ],
  s: 20
};

// ============================================================================
// Analysis of Attempt 5
// ============================================================================

// This works! But we had to:
// 1. Define each node type individually as an interface
// 2. Manually add 's: number' to each interface
// 3. Create the union manually

// PROBLEM: This doesn't scale!
// - We have 22 inline types in the real Pandoc AST
// - Many have complex nested structures
// - We'd have to duplicate every type definition
// - No reuse of the base definitions

// Can we do better? Can we somehow transform the base types programmatically?

// ============================================================================
// KEY FINDINGS SO FAR
// ============================================================================

/**
 * What we've learned:
 *
 * 1. SIMPLE INTERSECTION DOESN'T WORK (Attempt 1)
 *    type Annotated_Span = Inline_Span & { s: number }
 *    Problem: Doesn't recursively transform nested Inline[] to Annotated_Inline[]
 *
 * 2. PARAMETERIZED SELF-REFERENCE DOESN'T WORK (Attempt 2)
 *    type Inline_Span<T> = Base_Span<T>
 *    type X = Inline_Span<X>
 *    Problem: TypeScript rejects circular type alias references
 *
 * 3. UNION WITH RECURSION DOESN'T WORK (Attempt 4)
 *    type Inline = Str | Span<Inline>
 *    Problem: TypeScript rejects circular type alias references
 *
 * 4. INTERFACES WITH DIRECT UNION REFERENCES DO WORK (Attempt 5) ✓
 *    interface Span { c: Inline[] }
 *    type Inline = Str | Span
 *    Success: Recursion is "behind" an object property
 *
 * 5. KEY CONSTRAINT: TypeScript allows recursion when it's "behind" a property,
 *    but NOT when it's directly in a type parameter or union branch.
 *
 * NEXT QUESTIONS:
 * - Can we use mapped types to transform base types automatically?
 * - Can we use conditional types to detect and transform child fields?
 * - Can we create a generic transformation that works for all node types?
 */

// ============================================================================
// Attempt 6: Full parallel type hierarchies (explicit, no tricks)
// ============================================================================

// -----------------------------------------------------------------------------
// Base (non-annotated) type hierarchy
// -----------------------------------------------------------------------------

export interface Base_Inline_Str {
  t: "Str";
  c: string;
}

export interface Base_Inline_Span {
  t: "Span";
  classes: string[];
  c: Base_Inline[];
}

export type Base_Inline = Base_Inline_Str | Base_Inline_Span;

// -----------------------------------------------------------------------------
// Annotated type hierarchy (with s: number fields)
// -----------------------------------------------------------------------------

export interface Annotated_Inline_Str {
  t: "Str";
  c: string;
  s: number;
}

export interface Annotated_Inline_Span {
  t: "Span";
  classes: string[];
  c: Annotated_Inline[];
  s: number;
}

export type Annotated_Inline = Annotated_Inline_Str | Annotated_Inline_Span;

// ============================================================================
// Test: Can annotated types be used where base types are expected?
// ============================================================================

/**
 * Function that traverses the base AST and collects all string content
 */
function collectStrings(inline: Base_Inline): string[] {
  switch (inline.t) {
    case "Str":
      return [inline.c];
    case "Span":
      return inline.c.flatMap(collectStrings);
  }
}

/**
 * Function that traverses and collects class names from Spans
 */
function collectClasses(inline: Base_Inline): string[] {
  switch (inline.t) {
    case "Str":
      return [];
    case "Span":
      return [
        ...inline.classes,
        ...inline.c.flatMap(collectClasses)
      ];
  }
}

// -----------------------------------------------------------------------------
// Create test data
// -----------------------------------------------------------------------------

const baseStr: Inline_Str = { t: "Str", c: "hello" };
const baseSpan: Inline_Span = {
  t: "Span",
  classes: ["test"],
  c: [baseStr]
};

const annotatedStr: Annotated_Inline_Str = {
  t: "Str",
  c: "hello",
  s: 10
};

const annotatedSpan: Annotated_Inline_Span = {
  t: "Span",
  classes: ["emphasis"],
  c: [
    { t: "Str", c: "world", s: 20 },
    {
      t: "Span",
      classes: ["nested"],
      c: [{ t: "Str", c: "!", s: 30 }],
      s: 40
    }
  ],
  s: 50
};

// -----------------------------------------------------------------------------
// Test 1: Can we pass annotated values to functions expecting base types?
// -----------------------------------------------------------------------------

// CRITICAL TEST: Can we pass annotated values to base functions?
const stringsFromAnnotated = collectStrings(annotatedSpan);
const classesFromAnnotated = collectClasses(annotatedSpan);

// What about with explicit typing?
const annotatedAsBase: Base_Inline = annotatedStr;

// -----------------------------------------------------------------------------
// Test 2: Can we write generic functions that work with both?
// -----------------------------------------------------------------------------

function getNodeType<T extends { t: string }>(node: T): string {
  return node.t;
}

const typeFromBase = getNodeType(baseStr);
const typeFromAnnotated = getNodeType(annotatedStr);

// ============================================================================
// RESULTS: Full parallel hierarchies approach
// ============================================================================

/**
 * ✅ SUCCESS! The parallel hierarchies approach works!
 *
 * Key findings:
 *
 * 1. ANNOTATED TYPES ARE ASSIGNABLE TO BASE TYPES
 *    - `Annotated_Inline` can be passed to functions expecting `Base_Inline`
 *    - This is because TypeScript uses structural typing
 *    - `Annotated_Inline_Str` has all properties of `Base_Inline_Str` plus `s`
 *    - `Annotated_Inline_Span` has all properties of `Base_Inline_Span` plus `s`
 *
 * 2. ARRAY COVARIANCE WORKS FOR READ-ONLY OPERATIONS
 *    - `Annotated_Inline[]` is assignable to `Base_Inline[]` in read context
 *    - Our traversal functions (collectStrings, collectClasses) only READ
 *    - So they work perfectly with annotated types!
 *
 * 3. NO TYPE ASSERTIONS NEEDED
 *    - The assignment `const annotatedAsBase: Base_Inline = annotatedStr` works
 *    - No casts, no type assertions, no unsafe operations
 *
 * 4. GENERIC FUNCTIONS WORK WITH BOTH
 *    - Functions with constraints like `<T extends { t: string }>` work
 *    - Both hierarchies can be used with the same generic code
 *
 * IMPLICATIONS FOR FULL PANDOC AST:
 *
 * ✅ This approach scales! We can:
 *    - Define complete base types (Inline, Block, etc.)
 *    - Define complete annotated types (Annotated_Inline, Annotated_Block, etc.)
 *    - Write AST traversal functions that work on base types
 *    - Pass annotated ASTs to these functions without modification
 *    - All existing Pandoc-compatible code will work with annotated types
 *
 * ✅ Type safety is preserved:
 *    - Can't accidentally mix annotated and non-annotated in the same tree
 *    - But can safely "upcast" annotated to base when needed
 *    - Perfect for our use case: parse with annotations, use with existing code
 *
 * ⚠️  Trade-off: Code duplication
 *    - Must define every type twice (base and annotated)
 *    - 22 inline types × 2 = 44 type definitions
 *    - 15 block types × 2 = 30 type definitions
 *    - But: straightforward, predictable, maintainable
 *    - And: generated code can automate this
 *
 * NEXT STEPS:
 * - Apply this pattern to the full Pandoc AST
 * - Generate the parallel hierarchies from pandoc-types
 * - Test with real Pandoc operations
 */

// ============================================================================
// Corner Case Investigation: Attr and Target
// ============================================================================

/**
 * PROBLEM: Not all Pandoc data can have `s` fields added directly
 *
 * Two main categories of problematic structures:
 * 1. Tuple-based structures (arrays with fixed positions)
 * 2. Map keys (like Meta, already handled with metaTopLevelKeySources)
 *
 * TUPLE-BASED STRUCTURES IN PANDOC:
 *
 * 1. Attr = [string, string[], [string, string][]]
 *    - [id, classes, key-value attributes]
 *    - Used in: Code, Link, Image, Span, CodeBlock, Div, Header, Table, Figure,
 *      TableHead, TableBody, TableFoot, Row, Cell
 *    - Cannot add `s` fields to plain strings in arrays
 *
 * 2. Target = [string, string]
 *    - [url, title]
 *    - Used in: Link, Image
 *    - Cannot add `s` fields to plain strings in tuples
 *
 * SOLUTION: Parallel sideloaded structures
 *
 * For nodes containing tuple-based structures, add parallel `*S` fields
 * that mirror the structure with source IDs instead of strings.
 *
 * Example from user:
 * Input markdown: []{#id .class1 .class2 key1=value1}
 * Base Pandoc JSON: {"t":"Span","c":[["id",["class1","class2"],[["key1","value1"]]],[]]}
 *
 * Annotated JSON with source tracking:
 * {
 *   "t": "Span",
 *   "c": [["id",["class1","class2"],[["key1","value1"]]], []],
 *   "s": 0,     // Source ID for the Span node itself
 *   "attrS": [1, [2, 3], [[4, 5]]]  // Source IDs for Attr components
 * }
 *
 * Where:
 * - s: 0 → the entire <span> including brackets
 * - attrS[0]: 1 → the id string "id"
 * - attrS[1]: [2, 3] → classes "class1", "class2"
 * - attrS[2]: [[4, 5]] → key "key1", value "value1"
 */

// -----------------------------------------------------------------------------
// Type definitions for sideloaded source info
// -----------------------------------------------------------------------------

/**
 * Source information for Attr tuple: [id, classes, key-value pairs]
 * Mirrors the structure with source IDs (or null if empty/missing)
 */
type AttrSourceInfo = [
  number | null,                  // Source ID for id string (null if "")
  (number | null)[],              // Source IDs for each class
  [number | null, number | null][] // Source IDs for each [key, value] pair
];

/**
 * Source information for Target tuple: [url, title]
 * Mirrors the structure with source IDs
 */
type TargetSourceInfo = [
  number | null,  // Source ID for URL
  number | null   // Source ID for title
];

// -----------------------------------------------------------------------------
// Example: Annotated Span with Attr source tracking
// -----------------------------------------------------------------------------

type Attr = [string, string[], [string, string][]];

interface Annotated_Inline_Span_WithAttr {
  t: "Span";
  c: [Attr, Annotated_Inline[]];
  s: number;              // Source location of the entire Span node
  attrS: AttrSourceInfo;  // Source locations for each Attr component
}

// Test construction:
const spanWithAttrTracking: Annotated_Inline_Span_WithAttr = {
  t: "Span",
  c: [
    ["my-id", ["class1", "class2"], [["key1", "value1"]]],
    [{ t: "Str", c: "content", s: 6 }]
  ],
  s: 0,
  attrS: [1, [2, 3], [[4, 5]]]
};

// -----------------------------------------------------------------------------
// Example: Annotated Link with both Attr and Target tracking
// -----------------------------------------------------------------------------

type Target = [string, string];

interface Annotated_Inline_Link {
  t: "Link";
  c: [Attr, Annotated_Inline[], Target];
  s: number;               // Source location of the entire Link
  attrS: AttrSourceInfo;   // Source locations for Attr
  targetS: TargetSourceInfo; // Source locations for Target [url, title]
}

const linkWithTracking: Annotated_Inline_Link = {
  t: "Link",
  c: [
    ["link-id", ["external"], []],
    [{ t: "Str", c: "click here", s: 10 }],
    ["https://example.com", "Example Site"]
  ],
  s: 0,
  attrS: [1, [2], []],
  targetS: [3, 4]  // Source IDs for URL and title
};

// ============================================================================
// Analysis: Complete inventory of tuple-based structures
// ============================================================================

/**
 * COMPLETE LIST OF PANDOC NODE TYPES REQUIRING SIDELOADED SOURCE INFO:
 *
 * A. Inline types with Attr:
 *    - Code: [Attr, string]              → needs attrS
 *    - Link: [Attr, Inline[], Target]    → needs attrS + targetS
 *    - Image: [Attr, Inline[], Target]   → needs attrS + targetS
 *    - Span: [Attr, Inline[]]            → needs attrS
 *
 * B. Block types with Attr:
 *    - CodeBlock: [Attr, string]         → needs attrS
 *    - Header: [number, Attr, Inline[]]  → needs attrS
 *    - Table: [Attr, Caption, ...]       → needs attrS
 *    - Figure: [Attr, Caption, Block[]]  → needs attrS
 *    - Div: [Attr, Block[]]              → needs attrS
 *
 * C. Table components with Attr:
 *    - TableHead: [Attr, Row[]]          → needs attrS
 *    - TableBody: [Attr, RowHeadColumns, Row[], Row[]] → needs attrS
 *    - TableFoot: [Attr, Row[]]          → needs attrS
 *    - Row: [Attr, Cell[]]               → needs attrS
 *    - Cell: [Attr, Alignment, RowSpan, ColSpan, Block[]] → needs attrS
 *
 * D. Other tuple-based structures:
 *    - Target (in Link, Image): [url, title] → needs targetS
 *
 * E. Object-based structures (can add fields directly):
 *    - Citation: has citationId string → can add citationIdS: number directly
 *    - Caption: object with Inline[], Block[] → contents self-annotate
 *
 * DESIGN PATTERN SUMMARY:
 *
 * 1. Tuple-based structures (Attr, Target):
 *    - Add parallel `*S` field to containing node
 *    - Structure mirrors original with source IDs
 *    - Use null for empty/missing values
 *
 * 2. Object-based structures (Citation, Caption):
 *    - Add `*S` fields directly to object
 *    - Example: citationIdS for citationId
 *
 * 3. Map-based structures (Meta):
 *    - Already handled with metaTopLevelKeySources
 *    - Store key source IDs in top-level context
 */

// ============================================================================
// Verification: Does this approach work?
// ============================================================================

/**
 * ✅ YES! The sideloaded source info approach works because:
 *
 * 1. PRESERVES PANDOC STRUCTURE
 *    - Attr remains [string, string[], [string, string][]]
 *    - Target remains [string, string]
 *    - No breaking changes to base types
 *
 * 2. PROVIDES COMPLETE SOURCE TRACKING
 *    - Every user-entered string can be tracked
 *    - Fine-grained location info for diagnostics
 *    - Parallel structure is easy to navigate
 *
 * 3. MAINTAINS TYPE SAFETY
 *    - attrS and targetS are part of annotated types
 *    - Type system ensures they're present when needed
 *    - Optional in base types (not present)
 *
 * 4. COMPATIBLE WITH EXISTING CODE
 *    - Functions expecting base types ignore extra fields
 *    - Annotated types still assignable to base types
 *    - No runtime overhead for code that doesn't need source info
 *
 * 5. SCALES TO FULL PANDOC AST
 *    - ~15 node types need attrS
 *    - 2 node types need targetS
 *    - 1 type needs citationIdS
 *    - Straightforward, mechanical pattern
 *
 * ⚠️  IMPLEMENTATION NOTES:
 *
 * 1. Code generation:
 *    - Detect tuple-based structures in schema
 *    - Generate appropriate `*S` fields
 *    - Handle null values for empty strings
 *
 * 2. Serialization:
 *    - Include `*S` fields in JSON output
 *    - Omit if all values are null? (optimization)
 *
 * 3. Documentation:
 *    - Clear examples of parallel structures
 *    - Explain null semantics
 *    - Show traversal patterns
 */
