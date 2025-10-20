# SourceInfoPool Serialization Format Alternatives

## Current Format (Baseline)

### Original mapping
```json
{
  "id": 0,
  "range": {
    "start": {"offset": 0, "row": 0, "column": 0},
    "end": {"offset": 4, "row": 0, "column": 4}
  },
  "mapping": {
    "t": "Original",
    "c": {"file_id": 0}
  }
}
```
**Size: 158 bytes**

### Substring mapping
```json
{
  "id": 4,
  "range": {
    "start": {"offset": 0, "row": 0, "column": 0},
    "end": {"offset": 16, "row": 0, "column": 0}
  },
  "mapping": {
    "t": "Substring",
    "c": {"parent_id": 3, "offset": 4}
  }
}
```
**Size: 169 bytes**

### Concat mapping
```json
{
  "id": 11,
  "range": {
    "start": {"offset": 0, "row": 0, "column": 0},
    "end": {"offset": 5, "row": 0, "column": 0}
  },
  "mapping": {
    "t": "Concat",
    "c": {
      "pieces": [
        {"source_info_id": 9, "offset_in_concat": 0, "length": 4},
        {"source_info_id": 10, "offset_in_concat": 4, "length": 1}
      ]
    }
  }
}
```
**Size: 291 bytes**

---

## Alternative 1: Array-Based with Type Integer

Replace objects with arrays, use integer for type (0=Original, 1=Substring, 2=Concat, 3=Transformed).

Format: `[id, [start_offset, start_row, start_col, end_offset, end_row, end_col], type, ...type_data]`

### Original
```json
[0, [0, 0, 0, 4, 0, 4], 0, 0]
```
**Size: 30 bytes** (81% reduction)

### Substring
```json
[4, [0, 0, 0, 16, 0, 0], 1, 3, 4]
```
**Size: 34 bytes** (80% reduction)

### Concat
```json
[11, [0, 0, 0, 5, 0, 0], 2, [[9, 0, 4], [10, 4, 1]]]
```
**Size: 51 bytes** (82% reduction)

### Transformed
```json
[5, [0, 0, 0, 20, 0, 0], 3, 4, [[0, 10, 0, 10]]]
```
**Size: 50 bytes** (estimated)

**Pros:**
- Maximum space savings (80-82% reduction)
- Fast to parse (positional access)
- No field name overhead

**Cons:**
- Hardest to debug manually
- Requires careful documentation of positions
- Type safety only at runtime

---

## Alternative 2: Minimal Object with Short Keys

Keep objects but use 1-2 char keys.

Format:
- `i` = id
- `r` = range (array: [start_offset, start_row, start_col, end_offset, end_row, end_col])
- `t` = type (0-3)
- `d` = data (type-specific)

### Original
```json
{"i": 0, "r": [0, 0, 0, 4, 0, 4], "t": 0, "d": 0}
```
**Size: 48 bytes** (70% reduction)

### Substring
```json
{"i": 4, "r": [0, 0, 0, 16, 0, 0], "t": 1, "d": [3, 4]}
```
**Size: 54 bytes** (68% reduction)

### Concat
```json
{"i": 11, "r": [0, 0, 0, 5, 0, 0], "t": 2, "d": [[9, 0, 4], [10, 4, 1]]}
```
**Size: 69 bytes** (76% reduction)

### Transformed
```json
{"i": 5, "r": [0, 0, 0, 20, 0, 0], "t": 3, "d": [4, [[0, 10, 0, 10]]]}
```
**Size: 68 bytes** (estimated)

**Pros:**
- Good balance of size and readability
- Object structure easier to extend
- Still significant space savings

**Cons:**
- Short keys not self-documenting
- Slightly larger than pure array approach

---

## Alternative 3: Hybrid - Named Types with Array Data

Keep string type names but use arrays for data.

Format:
- `id` = id (keep for clarity in debugging)
- `r` = range array
- `m` = mapping type string
- `d` = data array

### Original
```json
{"id": 0, "r": [0, 0, 0, 4, 0, 4], "m": "O", "d": [0]}
```
**Size: 53 bytes** (66% reduction)

### Substring
```json
{"id": 4, "r": [0, 0, 0, 16, 0, 0], "m": "S", "d": [3, 4]}
```
**Size: 58 bytes** (66% reduction)

### Concat
```json
{"id": 11, "r": [0, 0, 0, 5, 0, 0], "m": "C", "d": [[9, 0, 4], [10, 4, 1]]}
```
**Size: 73 bytes** (75% reduction)

### Transformed
```json
{"id": 5, "r": [0, 0, 0, 20, 0, 0], "m": "T", "d": [4, [[0, 10, 0, 10]]]}
```
**Size: 72 bytes** (estimated)

**Pros:**
- Type names somewhat recognizable (O/S/C/T)
- Keeps `id` field for easier debugging
- Good space savings

**Cons:**
- Still need to remember data array positions
- Type strings add a few bytes vs integers

---

## Alternative 4: Compact Object with Single-Char Type Variants

Use single-char type, but keep type-specific data in named objects for clarity.

### Original
```json
{"i": 0, "r": [0, 0, 0, 4, 0, 4], "t": "O", "f": 0}
```
**Size: 49 bytes** (69% reduction)

### Substring
```json
{"i": 4, "r": [0, 0, 0, 16, 0, 0], "t": "S", "p": 3, "o": 4}
```
**Size: 59 bytes** (65% reduction)

### Concat
```json
{"i": 11, "r": [0, 0, 0, 5, 0, 0], "t": "C", "p": [[9, 0, 4], [10, 4, 1]]}
```
**Size: 73 bytes** (75% reduction)

### Transformed
```json
{"i": 5, "r": [0, 0, 0, 20, 0, 0], "t": "T", "p": 4, "m": [[0, 10, 0, 10]]}
```
**Size: 73 bytes** (estimated)

**Legend:**
- `i` = id, `r` = range, `t` = type
- `f` = file_id (Original)
- `p` = parent_id (Substring/Transformed), pieces (Concat)
- `o` = offset (Substring)
- `m` = mapping (Transformed)

**Pros:**
- Easier to debug than pure arrays
- Type-specific fields are still somewhat meaningful
- Good space savings

**Cons:**
- Field names still cryptic
- Slightly larger than pure array

---

## Alternative 5: Omit ID, Use Array Index

Since IDs are sequential (0, 1, 2...), we can omit them and use array index.

Format: Just `[range, type, data]` - ID is implicit from position in array.

### Original (at index 0)
```json
[[0, 0, 0, 4, 0, 4], 0, 0]
```
**Size: 25 bytes** (84% reduction)

### Substring (at index 4)
```json
[[0, 0, 0, 16, 0, 0], 1, 3, 4]
```
**Size: 29 bytes** (83% reduction)

### Concat (at index 11)
```json
[[0, 0, 0, 5, 0, 0], 2, [[9, 0, 4], [10, 4, 1]]]
```
**Size: 46 bytes** (84% reduction)

**Pros:**
- Maximum possible space savings
- Simplest structure
- ID lookup is just array indexing

**Cons:**
- Hardest to debug (no labels at all)
- Can't reorder pool without breaking references

---

## Recommendations

### For Maximum Compression: **Alternative 5** (84% reduction)
- Use array index as implicit ID
- Pure array format
- Best for production use where size matters most

### For Best Balance: **Alternative 2** (68-76% reduction)
- Short object keys (`i`, `r`, `t`, `d`)
- Object structure easier to debug
- Good middle ground

### For Easier Debugging: **Alternative 3** (66-75% reduction)
- Keep `id` field explicit
- Single-char type names (O/S/C/T)
- Array data but clearer structure

---

## Size Comparison Summary

| Format | Original | Substring | Concat | Avg Reduction |
|--------|----------|-----------|--------|---------------|
| **Current** | 158 | 169 | 291 | 0% |
| **Alt 1: Pure Array** | 30 | 34 | 51 | 81% |
| **Alt 2: Short Keys** | 48 | 54 | 69 | 71% |
| **Alt 3: Hybrid** | 53 | 58 | 73 | 69% |
| **Alt 4: Named Fields** | 49 | 59 | 73 | 70% |
| **Alt 5: No ID** | 25 | 29 | 46 | 84% |

---

## Implementation Complexity

**Easiest to Implement:** Alternative 2 or 3 (just change Serialize impl)

**Moderate:** Alternative 1 or 4 (manual serialization logic)

**Requires Care:** Alternative 5 (need to ensure ID assignment matches array index perfectly)

---

## My Recommendation

Start with **Alternative 2** (Short Keys):
- 70%+ space savings
- Still relatively debuggable
- Easy to implement with custom Serialize
- Can optimize to Alternative 5 later if needed

Or go straight to **Alternative 5** if you're confident about the ID ordering guarantee and want maximum compression.
