# Citation-Label Implementation Plan

**Date**: 2025-11-29
**Issue**: k-454
**Author**: Claude (with user guidance)

## Overview

The `citation-label` variable is a special auto-generated variable used in Harvard-style citations.
It produces compact labels like "Doe65", "RoNo78a" constructed from author names + year.

## Algorithm (from Pandoc citeproc)

### Base Label Generation

The base label (trigraph) is composed of: `namepart + yearpart`

**Name Part** (from `citationLabel` in Eval.hs:2798-2822):
1. Prefer `author` if present; otherwise use first available name variable
2. Fallback to "Xyz" if no names found
3. Take N characters from each author's family name, where N depends on author count:
   - **1 author**: 4 characters (e.g., "Asthma" → "Asth")
   - **2-3 authors**: 2 characters each (e.g., "Roe" + "Noakes" → "RoNo")
   - **4+ authors**: 1 character each, up to 4 authors (e.g., "Dipheria" + "Eczema" + "Flatulence" + "Goiter" → "DEFG")
4. For particles (von, de, etc.): use family name without particle for the letter extraction

**Year Part**:
1. Extract year from `issued` date
2. Format as 2 digits: `year % 100` with zero-padding (e.g., 2002 → "02", 1965 → "65")

**Examples from tests**:
- "Asthma, Albert (1900)" → "Asth" + "00" = "Asth00"
- "Roe + Noakes (1978)" → "Ro" + "No" + "78" = "RoNo78"
- "von Dipheria + Eczema + Flatulence + Goiter + Hiccups (1926)" → "D" + "E" + "F" + "G" + "26" = "DEFG26"

### Data Override

If the reference already has a `citation-label` field in its CSL-JSON data, use that instead of generating one.

### Year Suffix Interaction

The year suffix (a, b, c...) is appended during **rendering**, not during label generation:
- Base label: "Asth00"
- After disambiguation: "Asth00a", "Asth00b"

This matches how our existing `year-suffix` variable works.

## Architecture Design

### Option A: Compute on Demand (Recommended)

**Approach**: Generate citation-label lazily when the variable is requested.

**Pros**:
- Minimal code changes
- No storage overhead
- Follows existing pattern for computed variables like `page-first`

**Cons**:
- Computed on each access (minor performance concern)
- Need to check data override each time

**Implementation**:
1. Add `generate_citation_label()` function to `reference.rs`
2. Modify `Reference::get_variable()` to handle "citation-label" specially
3. Add year suffix handling in `evaluate_text()` similar to existing year-suffix logic

### Option B: Pre-compute and Store

**Approach**: Generate and store citation-label during reference loading or disambiguation.

**Pros**:
- Single computation
- Cached result

**Cons**:
- Requires storage field
- Must coordinate timing with data loading
- Adds complexity to reference lifecycle

### Recommendation: Option A

Option A is simpler and matches the pattern used for `page-first` computation. The computation is trivial (string slicing) and won't cause performance issues.

## Implementation Plan

### Phase 1: Core Label Generation

**File**: `crates/quarto-citeproc/src/reference.rs`

Add a function to generate the citation label:

```rust
impl Reference {
    /// Generate a citation label (trigraph) for this reference.
    ///
    /// Format: namepart + yearpart
    /// - 1 author: first 4 chars of family name
    /// - 2-3 authors: first 2 chars of each family name
    /// - 4+ authors: first 1 char of first 4 family names
    /// - Year: last 2 digits
    pub fn generate_citation_label(&self) -> String {
        let namepart = self.generate_citation_label_namepart();
        let yearpart = self.generate_citation_label_yearpart();
        format!("{}{}", namepart, yearpart)
    }

    fn generate_citation_label_namepart(&self) -> String {
        // Get author names, falling back to editor, translator, etc.
        let names = self.author.as_ref()
            .or(self.editor.as_ref())
            .or(self.translator.as_ref());

        let Some(names) = names else {
            return "Xyz".to_string();
        };

        if names.is_empty() {
            return "Xyz".to_string();
        }

        let chars_per_name = match names.len() {
            1 => 4,
            2 | 3 => 2,
            _ => 1,  // 4 or more authors
        };

        names.iter()
            .take(4)  // At most 4 names contribute
            .filter_map(|name| {
                // Get family name, stripping any embedded particle
                // e.g., "von Dipheria" -> "Dipheria"
                name.family.as_ref().map(|f| strip_particle(f))
            })
            .map(|family| {
                // Take first N chars, handling Unicode properly
                family.chars().take(chars_per_name).collect::<String>()
            })
            .collect()
    }

    /// Strip common particles from the beginning of a family name.
    ///
    /// This handles cases where particles are embedded in the family name
    /// (e.g., "von Dipheria") rather than being in the separate
    /// `non_dropping_particle` or `dropping_particle` fields.
    fn strip_particle(family: &str) -> String {
        // Common particles (lowercase)
        const PARTICLES: &[&str] = &[
            "von", "van", "de", "di", "da", "del", "della", "dello",
            "den", "der", "des", "du", "la", "le", "lo", "l'",
            "ten", "ter", "te", "auf", "zum", "zur", "vom",
        ];

        let lower = family.to_lowercase();
        for particle in PARTICLES {
            // Check for "particle " at start (with space)
            let prefix = format!("{} ", particle);
            if lower.starts_with(&prefix) {
                return family[prefix.len()..].to_string();
            }
            // Check for "particle'" at start (with apostrophe, like "l'")
            if particle.ends_with('\'') && lower.starts_with(particle) {
                return family[particle.len()..].to_string();
            }
        }
        family.to_string()
    }

    fn generate_citation_label_yearpart(&self) -> String {
        self.issued.as_ref()
            .and_then(|d| d.year())
            .map(|y| format!("{:02}", y.abs() % 100))
            .unwrap_or_default()
    }
}
```

### Phase 2: Variable Lookup Integration

**File**: `crates/quarto-citeproc/src/reference.rs`

Modify `get_variable()` to handle citation-label:

```rust
pub fn get_variable(&self, name: &str) -> Option<String> {
    match name {
        // ... existing cases ...

        "citation-label" => {
            // Check if explicitly provided in data first
            if let Some(label) = self.other.get("citation-label")
                .and_then(|v| v.as_str())
            {
                Some(label.to_string())
            } else {
                // Generate from author + year
                Some(self.generate_citation_label())
            }
        }

        // ... rest of match ...
    }
}
```

### Phase 3: Year Suffix Integration

**File**: `crates/quarto-citeproc/src/eval.rs`

Modify `evaluate_text()` to append year suffix for citation-label:

```rust
// In evaluate_text(), around line 680
if name == "citation-number" {
    // ... existing citation-number handling ...
} else if name == "citation-label" {
    // citation-label needs year suffix appended
    let base_label = ctx.get_variable("citation-label")?;
    let suffix_output = ctx.reference.disamb.as_ref()
        .and_then(|d| d.year_suffix)
        .map(|suffix| {
            let letter = year_suffix_to_letter(suffix);
            Output::tagged(Tag::YearSuffix(suffix), Output::literal(letter))
        })
        .unwrap_or(Output::null());

    let label_output = Output::literal(base_label);
    let result = if suffix_output.is_null() {
        label_output
    } else {
        Output::sequence(vec![label_output, suffix_output])
    };

    // Apply formatting and return
    return Ok(apply_formatting(result, &text_el.formatting));
}
```

### Phase 4: Add Tag for Citation Label

**File**: `crates/quarto-citeproc/src/output.rs`

Add a tag for citation-label (optional, for future extensibility):

```rust
pub enum Tag {
    // ... existing tags ...

    /// Citation label for Harvard-style citations
    CitationLabel,
}
```

## Testing Strategy

### Unit Tests

Add tests in `reference.rs`:

```rust
#[test]
fn test_citation_label_one_author() {
    let ref = Reference {
        author: Some(vec![Name { family: Some("Asthma".into()), .. }]),
        issued: Some(Date { year: 1900, .. }),
        ..
    };
    assert_eq!(ref.generate_citation_label(), "Asth00");
}

#[test]
fn test_citation_label_two_authors() {
    let ref = Reference {
        author: Some(vec![
            Name { family: Some("Roe".into()), .. },
            Name { family: Some("Noakes".into()), .. },
        ]),
        issued: Some(Date { year: 1978, .. }),
        ..
    };
    assert_eq!(ref.generate_citation_label(), "RoNo78");
}

#[test]
fn test_citation_label_four_plus_authors() {
    let ref = Reference {
        author: Some(vec![
            Name { family: Some("von Dipheria".into()), .. },  // Note: should use "Dipheria"
            Name { family: Some("Eczema".into()), .. },
            Name { family: Some("Flatulence".into()), .. },
            Name { family: Some("Goiter".into()), .. },
            Name { family: Some("Hiccups".into()), .. },
        ]),
        issued: Some(Date { year: 1926, .. }),
        ..
    };
    assert_eq!(ref.generate_citation_label(), "DEFG26");
}
```

### Integration Tests

Enable the 5 CSL conformance tests:
- disambiguate_CitationLabelDefault
- disambiguate_Trigraph
- disambiguate_CitationLabelInData
- magic_CitationLabelInCitation
- magic_CitationLabelInBibliography

## Edge Cases to Handle

1. **No authors**: Fall back to "Xyz"
2. **Short family names**: Take as many chars as available (e.g., "Li" with 4-char target → "Li")
3. **No year**: Empty year part (just namepart)
4. **Particles in names**: "von Dipheria" should use "D" not "v" - implemented via `strip_particle()` function
5. **Data override**: If `citation-label` is in the CSL-JSON, use it verbatim
6. **Unicode names**: Handle multi-byte characters correctly with `.chars().take(n)`
7. **Institutional names**: Names with `literal` but no `family` - may need to extract initial from literal

## Open Questions

1. **Particle handling**: ✓ Resolved - particles embedded in family names are stripped via pattern matching

2. **Sorting**: citation-label can be used in sort keys. Our existing sort key computation should handle this via `get_variable()`.

## Files to Modify

1. `crates/quarto-citeproc/src/reference.rs` - Add label generation
2. `crates/quarto-citeproc/src/eval.rs` - Add year suffix integration for citation-label
3. `crates/quarto-citeproc/src/output.rs` - (Optional) Add CitationLabel tag
4. `crates/quarto-citeproc/tests/enabled_tests.txt` - Enable the 5 tests

## Estimated Complexity

- **Low-Medium**: ~100-150 lines of new code
- **Risk**: Low - isolated feature with clear specification
- **Dependencies**: None - uses existing infrastructure
