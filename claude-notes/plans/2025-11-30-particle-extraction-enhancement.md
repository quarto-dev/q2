# Particle Extraction Enhancement Plan

**Status**: Completed
**Related Issue**: k-464
**Completed**: 2025-11-30

## Summary of Changes

The implementation addressed three tests for punctuation-connected particle extraction:
- `name_ParsedNonDroppingParticleWithApostrophe`
- `name_HyphenatedNonDroppingParticle1`
- `name_HyphenatedNonDroppingParticle2`

Plus a bonus fix for `position_IbidWithSuffix`.

### Key Changes Made

1. **Punctuation-connected particle extraction** (`reference.rs:365-391`)
   - Added fallback logic after space-based extraction
   - Handles apostrophe (`'`, `'`), hyphen (`-`), en-dash (`–`)
   - Example: "d'Aubignac" → non_dropping_particle="d'", family="Aubignac"
   - Example: "al-One" → non_dropping_particle="al-", family="One"

2. **No-space delimiter for punctuation particles** (`eval.rs:1474-1481`, `1565-1577`, `1671-1690`)
   - Added `ends_with_particle_punct()` helper function
   - When joining particle with family, no space is added if particle ends with punctuation
   - Per Haskell citeproc: `<+>` operator checks ending character

3. **Sort key generation fix** (`eval.rs:823-828`, `1303-1315`)
   - Set `in_sort_key = true` when computing sort keys via macro evaluation
   - Force inverted (family-first) order when `in_sort_key=true`
   - This ensures proper sorting by family name when particle is demoted

## Problem Statement

The current `extract_particles()` function in `reference.rs` only handles **space-separated** particles. It fails to extract particles that are connected by punctuation (apostrophes, hyphens, etc.).

### Failing Tests

| Test | Input | Expected Extraction |
|------|-------|---------------------|
| `name_ParsedDroppingParticleWithApostrophe` | `given: "François Hédelin d'"` | `given: "François Hédelin"`, `dropping_particle: "d'"` |
| `name_ParsedNonDroppingParticleWithApostrophe` | `family: "d'Aubignac"` | `family: "Aubignac"`, `non_dropping_particle: "d'"` |
| `name_HyphenatedNonDroppingParticle1` | `family: "al-One"` | `family: "One"`, `non_dropping_particle: "al-"` |
| `name_HyphenatedNonDroppingParticle2` | `family: "al-One"` | `family: "One"`, `non_dropping_particle: "al-"` |

## Current Implementation

Location: `crates/quarto-citeproc/src/reference.rs:306-364`

```rust
pub fn extract_particles(&mut self) {
    // Current: Only splits on whitespace
    let words: Vec<&str> = family.split_whitespace().collect();
    // ...finds particle words that are all lowercase
}
```

## Haskell Reference Implementation

Location: `external-sources/citeproc/src/Citeproc/Types.hs:1241-1281`

The Haskell implementation has a two-phase approach:

### Phase A: Try space-separated extraction
```haskell
case span (T.all isParticleChar) (T.words t) of
  (_,[])  -> name  -- no particle found
  (as,bs) -> name{ nameFamily = Just (T.unwords bs)
                 , nameNonDroppingParticle = Just (T.unwords as) }
```

### Phase B: If no space-separated particle, try punctuation-connected
```haskell
([],_) -> case T.split isParticlePunct t of
     [x,y] | T.all isParticleChar x ->
          name{ nameFamily = Just y
              , nameNonDroppingParticle = Just $ x <>
                  T.take 1 (T.dropWhile (not . isParticlePunct) t) }
     _ -> name
```

### Key Functions

```haskell
isParticlePunct c = c == '\'' || c == ''' || c == '-' || c == '\x2013' || c == '.'
isParticleChar c = isLower c || isParticlePunct c
```

**Particle punctuation characters:**
- `'` - straight apostrophe (U+0027)
- `'` - right single quote / curly apostrophe (U+2019)
- `-` - hyphen-minus (U+002D)
- `–` - en-dash (U+2013)
- `.` - period (U+002E)

## Proposed Implementation

### Step 1: Add punctuation constants

```rust
fn is_particle_punct(c: char) -> bool {
    matches!(c, '\'' | ''' | '-' | '\u{2013}' | '.')
}

fn is_particle_char(c: char) -> bool {
    c.is_lowercase() || is_particle_punct(c)
}
```

### Step 2: Enhance `extract_particles()` for non-dropping particles

After the existing space-based extraction fails, add punctuation-based extraction:

```rust
// If no space-separated particle found, try punctuation-connected
if self.non_dropping_particle.is_none() {
    if let Some(family) = self.family.clone() {
        // Try splitting on particle punctuation
        if let Some(punct_pos) = family.find(is_particle_punct) {
            let (before, after) = family.split_at(punct_pos);
            // Check if 'before' is all particle characters
            if !before.is_empty()
                && before.chars().all(is_particle_char)
                && after.len() > 1
            {
                // Include the punctuation with the particle
                let punct_char = after.chars().next().unwrap();
                self.non_dropping_particle = Some(format!("{}{}", before, punct_char));
                self.family = Some(after[punct_char.len_utf8()..].to_string());
            }
        }
    }
}
```

### Step 3: Enhance `extract_particles()` for dropping particles

Similarly for given names with trailing particles:

```rust
// If no space-separated particle found at end of given, try punctuation-connected
if self.dropping_particle.is_none() {
    if let Some(given) = self.given.clone() {
        // Look for trailing particle like "d'" at end
        // Find last punctuation that could start a particle
        if let Some(punct_pos) = given.rfind(|c| is_particle_punct(c) && c != '.') {
            let after_punct = &given[punct_pos..];
            // Check if everything after (and including) punctuation is particle-like
            if after_punct.chars().all(is_particle_char) {
                // Find word boundary before the particle
                let before = &given[..punct_pos];
                if let Some(space_pos) = before.rfind(char::is_whitespace) {
                    let particle_start = space_pos + 1;
                    let particle = &given[particle_start..];
                    if particle.chars().all(is_particle_char) {
                        self.given = Some(given[..space_pos].to_string());
                        self.dropping_particle = Some(particle.to_string());
                    }
                }
            }
        }
    }
}
```

## Testing Strategy

### Unit Tests to Add

1. **Apostrophe-connected non-dropping particle**
   - Input: `family: "d'Aubignac"`
   - Expected: `family: "Aubignac"`, `non_dropping_particle: "d'"`

2. **Hyphen-connected non-dropping particle**
   - Input: `family: "al-One"`
   - Expected: `family: "One"`, `non_dropping_particle: "al-"`

3. **Curly apostrophe**
   - Input: `family: "d'Aubignac"` (with U+2019)
   - Expected: Same extraction

4. **Apostrophe-connected dropping particle**
   - Input: `given: "François Hédelin d'"`
   - Expected: `given: "François Hédelin"`, `dropping_particle: "d'"`

5. **Mixed case (should NOT extract)**
   - Input: `family: "McDonald"`
   - Expected: No extraction (capital M after lowercase)

### CSL Conformance Tests to Enable

After implementation, enable:
- `name_ParsedDroppingParticleWithApostrophe`
- `name_ParsedNonDroppingParticleWithApostrophe`
- `name_HyphenatedNonDroppingParticle1`
- `name_HyphenatedNonDroppingParticle2`

## Risk Assessment

**Risk Level**: Medium

**Concerns:**
1. Could affect parsing of legitimate family names that contain hyphens or apostrophes (e.g., "O'Brien", "Müller-Schmidt")
2. Need to ensure we don't break existing passing tests

**Mitigations:**
1. Only extract when the prefix is ALL lowercase/particle characters
2. Run full test suite before and after
3. Consider adding an allowlist of known non-particle patterns

## Dependencies

- Phase 1 (name-part affixes wrapping) should be completed first
- This change affects `reference.rs` (parse-time), not `eval.rs` (render-time)

## Files to Modify

- `crates/quarto-citeproc/src/reference.rs` - `extract_particles()` function
- `crates/quarto-citeproc/tests/enabled_tests.txt` - Add newly passing tests
