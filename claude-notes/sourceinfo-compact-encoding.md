# Compact SourceInfo Encoding Specification

Format: `{"r": [start_offset, start_row, start_col, end_offset, end_row, end_col], "t": type_code, "d": data}`

ID is implicit from array index in the pool.

## Type Codes

- `0` = Original
- `1` = Substring
- `2` = Concat
- `3` = Transformed

## Type-Specific Data Encoding

### Type 0: Original

**Current format:**
```json
{
  "id": 0,
  "range": {"start": {"offset": 0, "row": 0, "column": 0}, "end": {"offset": 4, "row": 0, "column": 4}},
  "mapping": {"t": "Original", "c": {"file_id": 0}}
}
```

**Compact format:**
```json
{"r": [0, 0, 0, 4, 0, 4], "t": 0, "d": 0}
```

**Data field (`d`):** Just the file_id number (usize)

---

### Type 1: Substring

**Current format:**
```json
{
  "id": 4,
  "range": {"start": {"offset": 0, "row": 0, "column": 0}, "end": {"offset": 16, "row": 0, "column": 0}},
  "mapping": {"t": "Substring", "c": {"parent_id": 3, "offset": 4}}
}
```

**Compact format:**
```json
{"r": [0, 0, 0, 16, 0, 0], "t": 1, "d": [3, 4]}
```

**Data field (`d`):** `[parent_id, offset]`
- `parent_id`: Reference to parent SourceInfo in pool
- `offset`: Byte offset within parent

---

### Type 2: Concat

**Current format:**
```json
{
  "id": 11,
  "range": {"start": {"offset": 0, "row": 0, "column": 0}, "end": {"offset": 5, "row": 0, "column": 0}},
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

**Compact format:**
```json
{"r": [0, 0, 0, 5, 0, 0], "t": 2, "d": [[9, 0, 4], [10, 4, 1]]}
```

**Data field (`d`):** Array of pieces, where each piece is `[source_info_id, offset_in_concat, length]`
- `source_info_id`: Reference to piece's SourceInfo in pool
- `offset_in_concat`: Where this piece starts in the concatenated result
- `length`: Length of this piece

---

### Type 3: Transformed

**Current format:**
```json
{
  "id": 5,
  "range": {"start": {"offset": 0, "row": 0, "column": 0}, "end": {"offset": 20, "row": 0, "column": 0}},
  "mapping": {
    "t": "Transformed",
    "c": {
      "parent_id": 4,
      "mapping": [
        {"from_start": 0, "from_end": 10, "to_start": 0, "to_end": 10},
        {"from_start": 10, "from_end": 20, "to_start": 15, "to_end": 25}
      ]
    }
  }
}
```

**Compact format:**
```json
{"r": [0, 0, 0, 20, 0, 0], "t": 3, "d": [4, [[0, 10, 0, 10], [10, 20, 15, 25]]]}
```

**Data field (`d`):** `[parent_id, range_mappings]`
- `parent_id`: Reference to parent SourceInfo in pool
- `range_mappings`: Array of range mappings, where each is `[from_start, from_end, to_start, to_end]`

---

## Complete Example

### Full pool with all types:

```json
[
  {"r": [0, 0, 0, 10, 0, 10], "t": 0, "d": 0},
  {"r": [0, 0, 0, 5, 0, 5], "t": 0, "d": 0},
  {"r": [5, 0, 5, 10, 0, 10], "t": 0, "d": 0},
  {"r": [0, 0, 0, 5, 0, 5], "t": 1, "d": [0, 0]},
  {"r": [0, 0, 0, 5, 0, 5], "t": 1, "d": [0, 5]},
  {"r": [0, 0, 0, 10, 0, 10], "t": 2, "d": [[1, 0, 5], [2, 5, 5]]},
  {"r": [0, 0, 0, 8, 0, 8], "t": 3, "d": [0, [[0, 4, 0, 4], [4, 8, 6, 10]]]}
]
```

---

## Size Comparison

### Original format (158 bytes):
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

### Compact format (39 bytes):
```json
{"r": [0, 0, 0, 4, 0, 4], "t": 0, "d": 0}
```

**Reduction: 75%**

---

## Implementation Notes

### Serialization

Each `SerializableSourceInfo` needs custom serialization:
1. Omit the `id` field entirely
2. Serialize `range` as 6-element array `r`
3. Map variant name to type code `t`
4. Pack variant data into `d` according to type

### Deserialization

1. Array index becomes the ID
2. Deserialize `r` into Range struct
3. Use `t` to determine which variant to construct
4. Unpack `d` according to type code
5. Recursively resolve parent/piece references from pool

### Type Code Mapping

```rust
match type_code {
    0 => Original { file_id: data.as_u64() },
    1 => Substring {
        parent_id: data[0].as_u64(),
        offset: data[1].as_u64()
    },
    2 => Concat {
        pieces: data.as_array().map(|piece| [
            piece[0].as_u64(),  // source_info_id
            piece[1].as_u64(),  // offset_in_concat
            piece[2].as_u64(),  // length
        ])
    },
    3 => Transformed {
        parent_id: data[0].as_u64(),
        mapping: data[1].as_array().map(|rm| [
            rm[0].as_u64(),  // from_start
            rm[1].as_u64(),  // from_end
            rm[2].as_u64(),  // to_start
            rm[3].as_u64(),  // to_end
        ])
    },
}
```
