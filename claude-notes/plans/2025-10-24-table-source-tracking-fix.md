# Table Source Tracking - Pandoc Compatibility Fix

## Problem

We added `source_info` and `attr_source` fields to Table's structural components (Caption, Row, Cell, TableHead, TableBody, TableFoot), but serialized them as JSON objects with named fields. This breaks Pandoc compatibility because Pandoc expects these to be arrays in the "c" field.

**Pandoc's expected format:**
- Caption: `[shortCaption, longCaption]`
- TableHead: `[attr, rows]`
- Row: `[attr, cells]`
- Cell: `[attr, alignment, rowSpan, colSpan, content]`
- TableBody: `[attr, rowHeadColumns, head, body]`
- TableFoot: `[attr, rows]`

**What we were doing (WRONG):**
```json
{
  "t": "Table",
  "c": [attr, {"short": ..., "long": ..., "s": ...}, colspec, {...}, [...], {...}]
}
```

## Solution

Use **parallel source tracking fields** at the Table level, similar to how we handle `attrS` for attributes.

**New Table structure in JSON:**
```json
{
  "t": "Table",
  "s": <table-source-info>,
  "attrS": <table-attr-source>,
  "c": [
    attr,
    [shortCaption, longCaption],  // Keep as array
    colspec,
    [attr, rows],                  // TableHead as array
    [[attr, rhc, head, body], ...], // TableBody as array
    [attr, rows]                   // TableFoot as array
  ],
  "captionS": <caption-source-info>,
  "headS": {
    "s": <head-source-info>,
    "attrS": <head-attr-source>,
    "rowsS": [
      {
        "s": <row-source-info>,
        "attrS": <row-attr-source>,
        "cellsS": [
          {"s": <cell-source-info>, "attrS": <cell-attr-source>},
          ...
        ]
      },
      ...
    ]
  },
  "bodiesS": [
    {
      "s": <body-source-info>,
      "attrS": <body-attr-source>,
      "headS": [...],
      "bodyS": [...]
    },
    ...
  ],
  "footS": {
    "s": <foot-source-info>,
    "attrS": <foot-attr-source>,
    "rowsS": [...]
  }
}
```

## Implementation Steps

### 1. Rust Data Structures (table.rs)
Keep the Rust structs as-is - they already have `source_info` and `attr_source` fields. These are for internal use.

### 2. JSON Writer (writers/json.rs)
- Serialize Caption as `[short, long]` array in "c"
- Serialize TableHead as `[attr, rows]` array in "c"
- Serialize Row as `[attr, cells]` array in "c"
- Serialize Cell as `[attr, alignment, rowSpan, colSpan, content]` array in "c"
- Serialize TableBody as `[attr, rowHeadColumns, head, body]` array in "c"
- Serialize TableFoot as `[attr, rows]` array in "c"
- Add parallel fields: `captionS`, `headS`, `bodiesS`, `footS` to Table

### 3. JSON Reader (readers/json.rs)
- Parse Caption from `[short, long]` array
- Parse TableHead from `[attr, rows]` array
- Parse Row from `[attr, cells]` array
- Parse Cell from 5-element array
- Parse TableBody from 4-element array
- Parse TableFoot from 2-element array
- Read parallel source fields if present, use empty SourceInfo if not

### 4. TypeScript Types (ts-packages/annotated-qmd/src/pandoc-types.ts)
- Update Table interface to include parallel source fields
- Define RowSourceInfo, CellSourceInfo types
- Update existing interfaces to match new structure

## Benefits

1. **Pandoc Compatibility**: "c" field contains exact Pandoc format
2. **Source Tracking**: All source info preserved in parallel fields
3. **Backward Compatibility**: Missing source fields default to empty
4. **TypeScript Integration**: Clean types for accessing source info

## Related Issues

This follows the same pattern as:
- `attrS` for attribute source tracking
- `metaTopLevelKeySources` in astContext for YAML metadata
