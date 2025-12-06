# Project Naming Investigation

**Beads issue**: k-3n95
**Status**: In progress
**Date**: 2025-12-06

## Problem Statement

The Rust port of Quarto needs a distinct name to avoid confusion with Pandoc. The port will still use Pandoc in some codepaths, so clear naming is important to prevent user confusion about which tool is being referenced.

**Scope**: Internal codename during development, but should be feasible as a public-facing name if needed. Must avoid conflicts with large popular open-source projects and company names.

## Candidate Names

### ~~1. "kyoto"~~ — REJECTED

**Status**: Rejected due to cultural considerations

**Rejection reason**: Using a Japanese city name without personal/cultural connection to Japan raises cultural sensitivity concerns. The project lead prefers names with authentic cultural connections.

*(Original analysis preserved for reference)*

**Pros:**
- Short, memorable, easy to spell and pronounce
- No obvious negative connotations
- Pleasant sound

**Cons:**
- Major Japanese city - search results will be dominated by travel content
- Existing software: **Kyoto Cabinet** / **Kyoto Tycoon** (FAL Labs database project, largely dormant but still known in some circles)
- Cultural consideration: Using a Japanese city name for a project without Japanese cultural connection

---

### ~~2. "mate"~~ — NOT RECOMMENDED

**Status**: Investigated, not recommended due to major conflicts

**Major conflicts found:**
1. **MATE Desktop Environment** - Major Linux desktop environment (fork of GNOME 2), used by Ubuntu MATE and many distributions. Very well-known in open source. See: https://mate-desktop.org/
2. crates.io: **TAKEN** - "mate" is a job queue library (0.1.0-draft, ~1171 downloads)
3. npm: **TAKEN** - HTTP request library (last updated 2022)
4. PyPI: **TAKEN** - "Matchers for unittest" library

**Original appeal:**
- Short, culturally connected to southern Brazil (yerba mate)
- Nice connotations in English (companion, helper)

**Conclusion**: The MATE Desktop conflict alone is disqualifying for a public-facing open-source project name.

---

### 3. "pampa" — STRONG CANDIDATE

**Status**: Under active consideration

**Pros:**
- Short (5 letters), memorable, easy to spell
- Authentic cultural connection: Project lead is from Porto Alegre, Rio Grande do Sul, Brazil - part of the Pampas region
- Quechua origin meaning "flat surface" or "plain" - poetic resonance with plain text processing and Markdown's simplicity philosophy
- No major open-source project conflicts identified
- **crates.io: AVAILABLE** (critical for a Rust project)
- **PyPI: AVAILABLE**
- **GitHub org: AVAILABLE**

**Cons:**
- npm: TAKEN - "PAMPA – Protocol for Augmented Memory of Project Artifacts (MCP compatible)" - actively maintained
- Geographic term - some SEO competition with travel/geography content
- Used by some companies (Pampa Energía in Argentina)
- Less globally recognized than "Kyoto" (though this reduces SEO competition)

**Remaining investigation:**
- [ ] USPTO/EUIPO trademark search
- [ ] Domain availability (.dev, .io, .org)
- [ ] Homebrew formula name check

---

### 4. "pipoca" — STRONG CANDIDATE

**Status**: Under active consideration

**Etymology**: Brazilian Portuguese for "popcorn" — fun, playful, memorable.

**Pros:**
- 6 letters, memorable, distinctive
- Authentic cultural connection to Brazil
- Fun, playful connotation
- **crates.io: AVAILABLE**
- **npm: AVAILABLE**
- **PyPI: AVAILABLE**
- **GitHub org: AVAILABLE**
- All major package registries available (best availability of all candidates)

**Cons:**
- **Pipoca app exists** ([pipoca.app](https://pipoca.app/)) - Commercial movie/TV show guide app by Z3 Works, available on iOS and Android. Not open source, but notable presence in app ecosystem.
- GitHub user "pipoca" exists (but dormant since 2015, only 1 repo)
- [Pipoca Digital](https://github.com/pipocadigital) - Small GitHub org for web development projects (9 repos, mostly didactic)
- Various small Brazilian hobby projects use the name
- No semantic connection to text/document processing (unlike "pampa" = "flat/plain")
- May be less intuitive for non-Portuguese speakers initially
- Slightly longer than "pampa" (6 vs 5 letters)

**Remaining investigation:**
- [ ] USPTO/EUIPO trademark search
- [ ] Domain availability (.dev, .io, .org)
- [ ] Homebrew formula name check
- [ ] Assess severity of pipoca.app conflict

---

## Cultural Considerations

### Author's Background
- Born in Porto Alegre, Rio Grande do Sul, Brazil
- Resided in Minneapolis, USA for ~20 years
- Comfortable with names having cultural associations to southern Brazil
- Personal connection to authenticity matters more than personal meaning per se

### The Pampas Region
The Pampas are fertile lowlands covering:
- Southern Brazil (especially Rio Grande do Sul)
- Uruguay
- Argentina

This is a region with rich gaucho culture, known for cattle ranching, mate tea, and distinctive traditions.

### Naming Authenticity
Using names from Brazilian Portuguese/gaucho culture represents an authentic connection to the project creator's heritage, rather than appropriating a culture with no personal connection. This was a key factor in rejecting "kyoto."

---

## Investigation Results

### Package Registries

| Registry | "mate" | "pampa" | "pipoca" |
|----------|--------|---------|----------|
| crates.io | TAKEN (job queue) | **AVAILABLE** | **AVAILABLE** |
| npm | TAKEN (HTTP library) | TAKEN (MCP protocol) | **AVAILABLE** |
| PyPI | TAKEN (unittest matchers) | **AVAILABLE** | **AVAILABLE** |

### GitHub Namespaces

| Name | Org Available? | User Available? | Notes |
|------|----------------|-----------------|-------|
| mate | Yes | — | Brand conflict with MATE Desktop |
| pampa | **Yes** | — | Clean |
| pipoca | **Yes** | No (dormant) | User exists but inactive since 2015 |

### Major Project/Brand Conflicts

| Name | Conflict? | Severity | Details |
|------|-----------|----------|---------|
| mate | **YES** | **MAJOR** | MATE Desktop Environment (Linux desktop, fork of GNOME 2) |
| pampa | Minor | Low | npm package (MCP protocol tool), Pampa Energía (Argentine company) |
| pipoca | Moderate | Medium | [pipoca.app](https://pipoca.app/) - commercial movie/TV guide app |

---

## Open Questions (Answered)

1. ~~Is this intended as an internal codename or the public-facing name?~~
   **Answer**: Internal during development, but should be feasible for public-facing use.

2. ~~Are there other names from gaucho/southern Brazilian culture worth considering?~~
   **Answer**: "minuano" (cold wind) was considered but rejected due to pronunciation difficulty for English speakers. "pipoca" (popcorn) added as candidate.

3. ~~What weight should personal/cultural connection have vs. purely pragmatic considerations?~~
   **Answer**: Cultural authenticity matters (hence rejecting "kyoto"), but personal meaning is not a priority. Pragmatic concerns (namespace availability, avoiding conflicts) are important.

---

## Comparative Summary

| Criterion | mate | pampa | pipoca |
|-----------|------|-------|--------|
| crates.io | Taken | **Available** | **Available** |
| npm | Taken | Taken | **Available** |
| PyPI | Taken | **Available** | **Available** |
| GitHub org | Available* | **Available** | **Available** |
| Major OS project conflict | **MATE Desktop** | None | None |
| Commercial app conflict | None | Minor | **pipoca.app** |
| Cultural authenticity | Yes | **Yes** | **Yes** |
| Pronunciation (English) | Easy | Easy | Moderate** |
| Etymology meaning | "companion" | "flat/plain" | "popcorn" |
| Length | 4 | 5 | 6 |

\* mate GitHub org available but brand conflict with MATE Desktop
\** "pipoca" pronunciation: pee-POH-kah — learnable but not immediately obvious

### Analysis

**pampa advantages:**
- Etymology ("flat/plain") has poetic resonance with plain text processing
- Shorter
- Slightly easier pronunciation for English speakers
- Alliterative echo with "pandoc" — a nice phonetic nod given the project's relationship to Pandoc

**pipoca advantages:**
- All three major registries available (npm included)
- Fun, memorable, distinctive
- No direct software project conflicts (pipoca.app is in entertainment, not dev tools)

---

## Decision

**Chosen name: pampa**

**Date**: 2025-12-06

**Rationale:**
1. **Semantic resonance**: "Pampa" means "flat surface" or "plain" in Quechua — a poetic connection to plain text processing and Markdown's simplicity philosophy
2. **Alliterative echo**: "Pampa" and "Pandoc" share phonetic similarity, a fitting nod given that the project is a Rust port that will continue to interact with Pandoc in some codepaths
3. **Cultural authenticity**: Represents an authentic connection to the project creator's heritage (Porto Alegre, Rio Grande do Sul, Brazil — part of the Pampas region)
4. **Clean availability**: Available on crates.io (critical), PyPI, and GitHub org
5. **Pronunciation**: Intuitive for English speakers

**Remaining tasks before public launch (if applicable):**
- [ ] Check domain availability (.dev, .io, .org)
- [ ] USPTO/EUIPO trademark search
- [ ] Homebrew formula name check
- [ ] Reserve GitHub organization
- [ ] Reserve crates.io name (publish placeholder or reserve)
