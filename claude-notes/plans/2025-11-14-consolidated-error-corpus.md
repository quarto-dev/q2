# Plan: Consolidated Error Corpus Format

## Problem Analysis

### Current System

The error corpus uses Clinton Jeffery's approach (TOPLAS 2003): map (parser_state, lookahead_symbol) pairs to diagnostic messages.

**Current file structure:**
- `resources/error-corpus/NNN.qmd` - Test case content
- `resources/error-corpus/NNN.json` - Error metadata + capture coordinates
- One .qmd/.json pair per (state, sym) combination

**Current build process** (`scripts/build_error_table.ts`):
1. Glob all `.qmd` files in corpus directory
2. For each .qmd file:
   - Read corresponding .json file (error metadata)
   - Run parser with `--_internal-report-error-state`
   - Get `errorStates` and `tokens` from parser
   - Match captures (by row, column, size) to tokens
   - Augment captures with `lrState` and `sym` from tokens
   - Take first error state and combine with error info
   - Add entry to autogen table
3. Write `_autogen-table.json`
4. Macro generates Rust static array from JSON

**The duplication problem:**

Same error in different contexts hits different parser states:
- Q-2-10 (apostrophe/quote): **17 file pairs** (010-026)
  - All have identical: code, title, message, notes
  - Only differ in: test content, capture coordinates, resulting (state, sym)
  - States: 683, 735, 736, 666, 678, 654, 719, 666, 728, 708, 708, 708, 666, 709, 652, 682, 683
  - All have sym: "_whitespace"

Examples:
- `010.qmd`: `a' b.` → state 683
- `013.qmd`: `[a' b](url)` → state 666 (in link)
- `020.qmd`: `[--a' b.]` → state 708 (in editorial delete)

**Maintenance problems:**
1. Changing Q-2-10 message requires editing 17 JSON files
2. Risk of inconsistencies between copies
3. Unclear which numbered file tests which context
4. Adding new context = creating new numbered file pair

### Investigation Results

**Corpus statistics:**
- Total files: 41 .qmd files
- Q-2-10: 17 cases
- Q-2-7: 2 cases
- Q-2-5: 2 cases
- Others: 1 case each

**Autogen table structure:**
```json
[
  {
    "state": 683,
    "sym": "_whitespace",
    "row": 0,
    "column": 1,
    "errorInfo": {
      "code": "Q-2-10",
      "title": "Closed Quote Without Matching Open Quote",
      "message": "A space is causing...",
      "captures": [...],
      "notes": [...]
    },
    "name": "010"
  },
  // ... more entries
]
```

**Current Rust usage:**
- `lookup_error_entry()` iterates table, matches on (state, sym)
- Returns first match's error info
- All error info is duplicated per (state, sym) entry

## Proposed Solution

### New File Structure

**One JSON file per error code:**
- `resources/error-corpus/Q-2-10.json`
- `resources/error-corpus/Q-2-11.json`
- etc.

**New JSON schema:**
```json
{
  "code": "Q-2-10",
  "title": "Closed Quote Without Matching Open Quote",
  "message": "A space is causing a quote mark to be interpreted as a quotation close.",
  "notes": [
    {
      "message": "This is the opening quote. If you need an apostrophe, escape it with a backslash.",
      "label": "quote-start",
      "noteType": "simple"
    }
  ],
  "cases": [
    {
      "name": "simple-text",
      "description": "Apostrophe in plain text",
      "content": "a' b.",
      "captures": [
        {
          "label": "quote-start",
          "row": 0,
          "column": 1,
          "size": 1
        }
      ]
    },
    {
      "name": "in-link-text",
      "description": "Apostrophe inside link text",
      "content": "[a' b](url)",
      "captures": [
        {
          "label": "quote-start",
          "row": 0,
          "column": 2,
          "size": 1
        }
      ]
    },
    {
      "name": "in-editorial-delete",
      "description": "Apostrophe inside editorial delete markup",
      "content": "[--a' b.]",
      "captures": [
        {
          "label": "quote-start",
          "row": 0,
          "column": 4,
          "size": 1
        }
      ]
    }
  ]
}
```

**Key changes:**
- Error metadata (code, title, message, notes) appears ONCE
- New `cases` array contains test scenarios
- Each case has:
  - `name`: identifier (e.g., "simple-text", "in-link-text")
  - `description`: human-readable explanation of what's being tested
  - `content`: what was in the .qmd file
  - `captures`: coordinates relative to this content

### New Build Process

**Modified `scripts/build_error_table.ts`:**

```typescript
#!/usr/bin/env deno run --allow-read --allow-write --allow-env --allow-run

const result: any = [];
const tmpDir = Deno.makeTempDirSync();

try {
  // Glob Q-*.json files instead of .qmd files
  const files = Array.from(fs.globSync("resources/error-corpus/Q-*.json"))
    .toSorted((a, b) => a.localeCompare(b));

  for (const file of files) {
    console.log(`Processing ${file}`);
    const errorSpec = JSON.parse(Deno.readTextFileSync(file));
    const { code, title, message, notes, cases } = errorSpec;

    // Process each case in the cases array
    for (const testCase of cases) {
      const { name, content, captures } = testCase;

      // Write content to temporary .qmd file
      const tmpFile = `${tmpDir}/${code}-${name}.qmd`;
      Deno.writeTextFileSync(tmpFile, content);

      // Run parser with error state reporting
      const parseResult = new Deno.Command("../../target/debug/quarto-markdown-pandoc", {
        args: ["--_internal-report-error-state", "-i", tmpFile],
      });
      const output = await parseResult.output();
      const outputStdout = new TextDecoder().decode(output.stdout);
      const parseResultJson = JSON.parse(outputStdout);
      const { errorStates, tokens } = parseResultJson;

      if (errorStates.length < 1) {
        throw new Error(`Expected at least one error state for ${code}/${name}`);
      }

      // Match and augment captures (same logic as before)
      const looseMatching = captures.some((e: any) => e.size === undefined);
      const matches = looseMatching ?
        leftJoin(tokens, captures, (tok: any, cap: any) =>
          tok.row === cap.row && tok.column === cap.column &&
          (cap.size !== undefined ? tok.size === cap.size : true))
        : leftKeyJoin(tokens, captures, (e: any) =>
          e.size ? `${e.row}:${e.column}:${e.size}` : `${e.row}:${e.column}`);

      const augmentedCaptures = captures.map((capture: any) => {
        const match = matches.find(([, b]) => b === capture);
        assert(match);
        return {...match[0], ...match[1]};
      });

      // Create autogen table entry
      result.push({
        ...errorStates[0],
        errorInfo: {
          code,
          title,
          message,
          captures: augmentedCaptures,
          notes
        },
        name: `${code}/${name}`,
      });
    }
  }
} finally {
  // Clean up temp directory
  Deno.removeSync(tmpDir, { recursive: true });
}

// Rest stays the same
Deno.writeTextFileSync("resources/error-corpus/_autogen-table.json",
  JSON.stringify(result, null, 2) + "\n");
// ... touch source file, rebuild
```

**Key changes:**
- Glob for `Q-*.json` instead of `.qmd` files
- Parse error spec with cases array
- For each case, write content to temp file, run parser
- Generate one autogen entry per case
- Clean up temp files

**Autogen table format unchanged:**
- Still one entry per (state, sym) pair
- Still has full error info embedded
- Rust code requires NO changes
- `include_error_table!` macro works as-is

## Implementation Steps

### Phase 1: Design & Validate (1-2 hours)

1. **Define JSON Schema**
   - Write formal schema for new Q-*.json format
   - Document all fields (including optional ones)
   - Create schema validation function

2. **Create Example**
   - Manually create `Q-2-10.json` with all 17 cases
   - Verify it captures all contexts from 010-026

3. **Write Validation Tests**
   - Test schema validation
   - Test that examples parse correctly

### Phase 2: Build Script Modification (2-3 hours)

1. **Modify `build_error_table.ts`**
   - Change file globbing
   - Add temp file handling
   - Process cases array
   - Add better error messages
   - Add progress indicators

2. **Test with Example**
   - Run modified script on Q-2-10.json
   - Verify autogen table entries match old 010-026 entries
   - Compare (state, sym) pairs

3. **Add Backwards Compatibility**
   - Support old .qmd/.json format during migration
   - Allow mixed corpus (some old, some new)

### Phase 3: Migration Script (2-3 hours)

1. **Write `scripts/migrate_error_corpus.ts`**
   ```typescript
   // Read all NNN.json files
   // Group by error code
   // For each group:
   //   - Extract shared metadata
   //   - Create cases array from individual files
   //   - Generate case names from content analysis
   //   - Write Q-*.json file
   // Generate migration report
   ```

2. **Case Name Generation**
   - Analyze content to generate meaningful names
   - Examples:
     - `a' b.` → "simple-text"
     - `[a' b](url)` → "in-link-text"
     - `**a' b**` → "in-emphasis"
     - `[--a' b]` → "in-editorial-delete"

3. **Validation**
   - Run old build, save autogen table
   - Run migration
   - Run new build, save autogen table
   - Compare tables (should be identical except "name" field)

### Phase 4: Migration Execution (1 hour)

1. **Backup**
   - Create `resources/error-corpus/old/` directory
   - Copy all NNN.{qmd,json} files there

2. **Run Migration**
   - Execute migration script
   - Generate Q-*.json files
   - Run new build script
   - Compare autogen tables

3. **Verify**
   - Check that all error codes are present
   - Check that all (state, sym) pairs are preserved
   - Run Rust tests
   - Run end-to-end parser tests

### Phase 5: Cleanup & Documentation (1 hour)

1. **Remove Old Files**
   - Delete numbered .qmd/.json files from corpus
   - Keep old/ backup temporarily

2. **Update Documentation**
   - Update CLAUDE.md in quarto-markdown-pandoc
   - Explain new corpus format
   - Document how to add new cases
   - Document how to add new error codes

3. **Update Scripts**
   - Remove backwards compatibility code
   - Add helper scripts:
     - `add_error_case.ts` - add case to existing error
     - `new_error_code.ts` - create new Q-*.json file

### Phase 6: Enhancement Opportunities (Future)

1. **Case Name Index**
   - Build reverse index: case name → (state, sym)
   - Useful for debugging: "which case triggered this state?"

2. **Coverage Analysis**
   - Track which cases actually get hit in real-world corpus
   - Identify redundant cases (same state, sym)
   - Identify missing contexts

3. **Auto-generate Cases**
   - Given error example in wild, suggest case name
   - Check if (state, sym) already covered
   - Generate new case JSON

## Benefits

### Immediate Benefits

1. **Single Source of Truth**
   - Error message in one place
   - Change once, applies to all cases
   - No inconsistency risk

2. **Maintainability**
   - Easy to see all contexts for an error
   - Clear case names document what's tested
   - Simple to add new context

3. **Clarity**
   - Case names explain parser state differences
   - Descriptions document intent
   - File names match error codes

### Long-term Benefits

1. **Scalability**
   - Easy to expand coverage
   - Each error can have dozens of cases
   - No file proliferation

2. **Documentation**
   - Corpus becomes self-documenting
   - Case names serve as test documentation
   - Easy to understand coverage

3. **Tooling**
   - Can build case management tools
   - Can analyze coverage
   - Can auto-suggest new cases

## Risks & Mitigations

### Risk: Migration introduces bugs

**Mitigation:**
- Extensive validation comparing old vs new autogen tables
- Keep old files in backup directory
- Run full test suite before/after
- Can roll back by reverting build script

### Risk: Build script complexity increases

**Mitigation:**
- Temp file handling is straightforward
- Error messages improved
- Code is well-documented
- Can keep old script as reference

### Risk: Developers add cases incorrectly

**Mitigation:**
- Schema validation catches errors
- Helper scripts simplify addition
- Documentation with examples
- Build script provides clear errors

## Open Questions

1. **Case naming convention?**
   - Proposal: kebab-case, descriptive
   - Examples: "simple-text", "in-link-text", "in-emphasis"
   - Allow flexibility as contexts are domain-specific

2. **How to handle future parser changes?**
   - Parser states may change with grammar updates
   - Cases stay valid (content + captures)
   - Rebuild will generate new (state, sym) pairs
   - May need to retire cases that no longer error

3. **Should we keep any .qmd files?**
   - Proposal: No, content lives in JSON
   - Easier to manage single format
   - Can always extract to temp file

4. **Backwards compatibility period?**
   - Proposal: Support both formats for one release
   - Then remove old format
   - Keeps rollback option open

## Success Criteria

1. ✓ All existing (state, sym) pairs preserved in autogen table
2. ✓ All Rust tests pass
3. ✓ Parser produces identical errors on real-world files
4. ✓ Q-2-10 consolidated from 17 files to 1 file
5. ✓ Documentation updated
6. ✓ Helper scripts created
7. ✓ Migration validated and backed up

## Timeline Estimate

- Phase 1: 1-2 hours
- Phase 2: 2-3 hours
- Phase 3: 2-3 hours
- Phase 4: 1 hour
- Phase 5: 1 hour
- **Total: 7-10 hours**

Can be done incrementally:
- Day 1: Phases 1-2 (design + build script)
- Day 2: Phase 3 (migration script)
- Day 3: Phases 4-5 (execute + cleanup)
