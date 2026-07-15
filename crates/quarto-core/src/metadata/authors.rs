/*
 * metadata/authors.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Typed author/affiliation model parsed from document metadata.
 */

//! Typed author/affiliation model.
//!
//! Parses the `author` / `authors` / `affiliations` / `funding`
//! metadata keys into typed structs, mirroring the normalization
//! Quarto 1 performs in `src/resources/filters/modules/authors.lua`:
//! structured names, degrees, contact fields, ORCID, attribute flags,
//! CRediT roles, affiliations (inline, `ref:`-referenced, and the
//! top-level `affiliations:` block) with de-duplication and stable
//! `aff-N` ids, and funding (schema normalization only — nothing in
//! HTML consumes it, per design decision Q7).
//!
//! Phase 2 of the title-block parity epic (bd-gx9cic8z / bd-ez0hiowa).
//! Plan: `claude-notes/plans/2026-07-15-html-title-block-parity.md`.
//!
//! ## Documented deviations from Q1's `authors.lua`
//!
//! - **Name splitting**: Q1 round-trips a literal name through
//!   Pandoc's BibTeX reader to derive `given`/`family`. We implement
//!   the equivalent BibTeX "First von Last" / "von Last, First"
//!   heuristic directly (lowercase-starting tokens form the particle
//!   span). Like Q1, a derived dropping particle is stored as
//!   non-dropping.
//! - **Name literals include particles**: Q1's `createNameLiteral`
//!   joins only `given family`; we join
//!   `given dropping-particle non-dropping-particle family`
//!   ("Vincent van Gogh", not "Vincent Gogh").
//! - **Letters**: Q1's `letter()` is `number % 26` (breaking at the
//!   26th entry); we use proper base-26 (`a`..`z`, `aa`, `ab`, …).
//! - **Undefined `ref:`**: Q1 aborts the render; we drop the
//!   reference and record the problem in [`AuthorsModel::issues`] so
//!   the caller can surface a diagnostic.
//! - **Roles maps**: `roles: {researcher: lead, writer: supporting}`
//!   takes every entry (Q1 silently keeps only the first).
//! - **`institute`/`institutes`** (revealjs/beamer) is not handled
//!   yet; it can join when those formats need it.

use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};

/// The normalized author model for a document.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AuthorsModel {
    /// Normalized authors, in document order.
    pub authors: Vec<Author>,
    /// Normalized affiliations, in first-reference order.
    pub affiliations: Vec<Affiliation>,
    /// Normalized funding groups (schema only; no HTML output).
    pub funding: Vec<FundingGroup>,
    /// Human-readable problems found during normalization (e.g. an
    /// author referencing an undefined affiliation). Q1 aborts on
    /// these; we drop the offending piece and report.
    pub issues: Vec<String>,
}

/// One document author.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Author {
    /// Explicit `id:` if given; otherwise `None` (consumers fall back
    /// to [`Author::number`], like Q1).
    pub id: Option<String>,
    /// 1-based position among the authors.
    pub number: usize,
    /// Base-26 letter form of `number` (`a`, `b`, …, `z`, `aa`, …).
    pub letter: String,
    /// The author's name.
    pub name: AuthorName,
    /// Home page.
    pub url: Option<String>,
    /// Email address.
    pub email: Option<String>,
    /// Phone number.
    pub phone: Option<String>,
    /// Fax number (Q1 schema carries it; so do we).
    pub fax: Option<String>,
    /// ORCID identifier (bare id, not the orcid.org URL).
    pub orcid: Option<String>,
    /// Acknowledgements text.
    pub acknowledgements: Option<String>,
    /// Academic titles displayed after the name ("PhD", "MD").
    pub degrees: Vec<String>,
    /// Author note (globally numbered across the document's authors).
    pub note: Option<AuthorNote>,
    /// Attribute flags that are true for this author
    /// (`corresponding`, `equal-contributor`, `deceased`, or any
    /// custom flag from an `attributes:` list).
    pub attributes: Vec<String>,
    /// Contributor roles (CRediT-normalized where recognized).
    pub roles: Vec<AuthorRole>,
    /// Ids into [`AuthorsModel::affiliations`].
    pub affiliations: Vec<String>,
    /// Unrecognized keys, bucketed like Q1's `metadata` field.
    pub metadata: Vec<ConfigMapEntry>,
}

/// An author's name.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AuthorName {
    /// The full display name ("Norah Jones").
    pub literal: String,
    /// Given name(s).
    pub given: Option<String>,
    /// Family name.
    pub family: Option<String>,
    /// CSL dropping particle.
    pub dropping_particle: Option<String>,
    /// CSL non-dropping particle ("van", "de la").
    pub non_dropping_particle: Option<String>,
}

/// An author note, numbered across the document's authors.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthorNote {
    /// 1-based note number.
    pub number: usize,
    /// Note text.
    pub text: String,
}

/// A contributor role, optionally CRediT-decorated.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AuthorRole {
    /// The role as written ("conceptualization", "writing").
    pub role: String,
    /// Degree of contribution ("lead", "supporting").
    pub degree_of_contribution: Option<String>,
    /// CRediT vocabulary identifier, when the role is a CRediT term.
    pub vocab_identifier: Option<String>,
    /// Canonical CRediT term.
    pub vocab_term: Option<String>,
    /// CRediT term URL.
    pub vocab_term_identifier: Option<String>,
}

/// One affiliation.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Affiliation {
    /// Explicit `id:` or assigned `aff-N`.
    pub id: String,
    /// 1-based position among the affiliations.
    pub number: usize,
    /// Base-26 letter form of `number`.
    pub letter: String,
    /// Institution name.
    pub name: Option<String>,
    /// Department.
    pub department: Option<String>,
    /// Group / lab.
    pub group: Option<String>,
    /// Street address.
    pub address: Option<String>,
    /// City.
    pub city: Option<String>,
    /// Region (aliased from `state:` too).
    pub region: Option<String>,
    /// Country.
    pub country: Option<String>,
    /// Postal code.
    pub postal_code: Option<String>,
    /// Home page (aliased from `affiliation-url:` too).
    pub url: Option<String>,
    /// ISNI identifier.
    pub isni: Option<String>,
    /// Ringgold identifier.
    pub ringgold: Option<String>,
    /// ROR identifier.
    pub ror: Option<String>,
    /// Unrecognized keys.
    pub metadata: Vec<ConfigMapEntry>,
}

/// One funding group (one entry of the `funding:` list).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FundingGroup {
    /// Funding statement text.
    pub statement: Option<String>,
    /// Open-access statement text.
    pub open_access: Option<String>,
    /// Awards.
    pub awards: Vec<Award>,
}

/// One award within a funding group.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Award {
    /// Award id / grant number.
    pub id: Option<String>,
    /// Award name.
    pub name: Option<String>,
    /// Award description.
    pub description: Option<String>,
    /// Funding sources.
    pub source: Vec<FundingSource>,
    /// Recipients (people or institutions).
    pub recipient: Vec<FundingParty>,
    /// Investigators.
    pub investigator: Vec<FundingParty>,
}

/// A funding source: a party plus Q1's per-source decorations.
#[derive(Debug, Clone, PartialEq)]
pub struct FundingSource {
    /// Who funds.
    pub party: FundingParty,
    /// Country of the source.
    pub country: Option<String>,
    /// Source type ("federal", …).
    pub source_type: Option<String>,
}

/// A person or institution in a funding entry. `ref:` values resolve
/// against the document's authors and affiliations.
#[derive(Debug, Clone, PartialEq)]
pub enum FundingParty {
    /// Free text.
    Text(String),
    /// A person (inline `name:` or a resolved author `ref:`).
    Name(AuthorName),
    /// An institution (inline `institution:` or a resolved
    /// affiliation `ref:`).
    Institution(Box<Affiliation>),
}

// ─────────────────────────────────────────────────────────────────────
// Parsing
// ─────────────────────────────────────────────────────────────────────

/// Parse the document's full author/affiliation/funding model.
///
/// Reads, per Quarto 1 semantics, the `author`/`authors` keys (first
/// key with a non-empty result wins), the top-level `affiliations:`
/// block, and the `funding:` key.
pub fn parse_authors_model(meta: &ConfigValue) -> AuthorsModel {
    let mut model = AuthorsModel::default();
    let mut note_counter = 0usize;

    for key in ["author", "authors"] {
        if let Some(value) = meta.get(key) {
            let entries: Vec<&ConfigValue> = if let Some(arr) = value.as_array() {
                arr.iter().collect()
            } else {
                vec![value]
            };
            let affiliations_before = model.affiliations.clone();
            let mut authors = Vec::new();
            for entry in entries {
                if let Some(author) =
                    parse_author_entry(entry, &mut model.affiliations, &mut note_counter)
                {
                    authors.push(author);
                }
            }
            if !authors.is_empty() {
                model.authors = authors;
                break;
            }
            // A key that produced no authors must not leak inline
            // affiliations from its dropped entries.
            model.affiliations = affiliations_before;
        }
    }

    // Top-level `affiliations:` block. Entries that duplicate an
    // affiliation already collected from an author are merged; any
    // author refs to the duplicate's explicit id are remapped.
    if let Some(value) = meta.get("affiliations") {
        for aff in parse_affiliation_values(value) {
            let original_id = aff.id.clone();
            let merged_id = maybe_add_affiliation(aff, &mut model.affiliations);
            if !original_id.is_empty() && original_id != merged_id {
                for author in &mut model.authors {
                    for r in &mut author.affiliations {
                        if *r == original_id {
                            *r = merged_id.clone();
                        }
                    }
                }
            }
        }
    }

    // Validate refs: drop (and report) references to undefined
    // affiliations. Q1 aborts the render here; see module docs.
    let known: Vec<String> = model.affiliations.iter().map(|a| a.id.clone()).collect();
    let mut ref_issues = Vec::new();
    for author in &mut model.authors {
        let author_name = author.name.literal.clone();
        author.affiliations.retain(|r| {
            let ok = known.contains(r);
            if !ok {
                ref_issues.push(format!(
                    "Undefined affiliation '{r}' for author '{author_name}'."
                ));
            }
            ok
        });
    }
    model.issues.extend(ref_issues);

    // Number the authors and affiliations.
    for (i, author) in model.authors.iter_mut().enumerate() {
        author.number = i + 1;
        author.letter = letter(i + 1);
    }
    for (i, aff) in model.affiliations.iter_mut().enumerate() {
        aff.number = i + 1;
        aff.letter = letter(i + 1);
    }

    // Funding (schema normalization only). Parsed last so `ref:`
    // values resolve against the finished authors/affiliations.
    if let Some(value) = meta.get("funding") {
        let groups: Vec<&ConfigValue> = if let Some(arr) = value.as_array() {
            arr.iter().collect()
        } else {
            vec![value]
        };
        let mut funding = Vec::new();
        let mut issues = Vec::new();
        for group in groups {
            funding.push(parse_funding_group(
                group,
                &model.authors,
                &model.affiliations,
                &mut issues,
            ));
        }
        model.funding = funding;
        model.issues.extend(issues);
    }

    model
}

/// Parse just the authors (compatibility wrapper over
/// [`parse_authors_model`]).
pub fn parse_authors(meta: &ConfigValue) -> Vec<Author> {
    parse_authors_model(meta).authors
}

/// Base-26 letter form of a 1-based number: `a`..`z`, `aa`, `ab`, …
fn letter(mut number: usize) -> String {
    let mut out = Vec::new();
    while number > 0 {
        number -= 1;
        out.push(b'a' + (number % 26) as u8);
        number /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Author fields that are plain-text scalars.
const AUTHOR_SIMPLE_FIELDS: &[&str] =
    &["url", "email", "fax", "phone", "orcid", "acknowledgements"];

/// Author fields that are attribute flags when truthy.
const AUTHOR_ATTRIBUTE_FIELDS: &[&str] = &["corresponding", "equal-contributor", "deceased"];

/// Top-level name-component fields accepted directly on an author.
const AUTHOR_NAME_COMPONENT_FIELDS: &[&str] = &[
    "given",
    "family",
    "literal",
    "dropping-particle",
    "non-dropping-particle",
];

/// Parse one author entry (scalar name or map). Inline affiliation
/// definitions are collected (with de-duplication) into
/// `affiliations`.
fn parse_author_entry(
    value: &ConfigValue,
    affiliations: &mut Vec<Affiliation>,
    note_counter: &mut usize,
) -> Option<Author> {
    let mut author = Author::default();

    if let Some(text) = value.as_plain_text() {
        author.name = to_name_from_literal(&text);
        if author.name.literal.is_empty() {
            return None;
        }
        return Some(author);
    }

    let entries = value.as_map_entries()?;
    let mut name_components: Vec<(&str, String)> = Vec::new();
    let mut affiliation_url: Option<String> = None;

    for entry in entries {
        let key = entry.key.as_str();
        let val = &entry.value;
        match key {
            "name" => author.name = to_name(val),
            "id" => author.id = val.as_plain_text(),
            _ if AUTHOR_SIMPLE_FIELDS.contains(&key) => {
                let text = val.as_plain_text();
                match key {
                    "url" => author.url = text,
                    "email" => author.email = text,
                    "fax" => author.fax = text,
                    "phone" => author.phone = text,
                    "orcid" => author.orcid = text,
                    "acknowledgements" => author.acknowledgements = text,
                    _ => unreachable!(),
                }
            }
            _ if AUTHOR_ATTRIBUTE_FIELDS.contains(&key) => {
                if val.as_bool().unwrap_or(false)
                    || val
                        .as_plain_text()
                        .is_some_and(|t| t.eq_ignore_ascii_case("true"))
                {
                    set_attribute(&mut author, key);
                }
            }
            "attributes" => parse_attributes(&mut author, val),
            "note" => {
                // Accept plain text or a round-tripped `{number, text}` map.
                let text = val
                    .as_plain_text()
                    .or_else(|| val.get("text").and_then(|t| t.as_plain_text()));
                if let Some(text) = text {
                    *note_counter += 1;
                    author.note = Some(AuthorNote {
                        number: *note_counter,
                        text,
                    });
                }
            }
            "affiliation" | "affiliations" => {
                let parsed = parse_author_affiliations(val, &mut author);
                for aff in parsed {
                    let id = maybe_add_affiliation(aff, affiliations);
                    author.affiliations.push(id);
                }
            }
            "affiliation-url" => affiliation_url = val.as_plain_text(),
            "role" | "roles" => parse_roles(&mut author, val),
            "degrees" => {
                if let Some(arr) = val.as_array() {
                    author
                        .degrees
                        .extend(arr.iter().filter_map(|d| d.as_plain_text()));
                } else if let Some(d) = val.as_plain_text() {
                    author.degrees.push(d);
                }
            }
            _ if AUTHOR_NAME_COMPONENT_FIELDS.contains(&key) => {
                if let Some(text) = val.as_plain_text() {
                    name_components.push((key, text));
                }
            }
            _ => author.metadata.push(entry.clone()),
        }
    }

    // Q1: top-level name-component fields fold into the name.
    if !name_components.is_empty() {
        for (key, text) in name_components {
            match key {
                "given" => author.name.given = Some(text),
                "family" => author.name.family = Some(text),
                "literal" => author.name.literal = text,
                "dropping-particle" => author.name.dropping_particle = Some(text),
                "non-dropping-particle" => author.name.non_dropping_particle = Some(text),
                _ => unreachable!(),
            }
        }
        if author.name.literal.is_empty() {
            author.name.literal = compose_literal(&author.name);
        }
    }

    // `affiliation-url:` decorates the author's first affiliation.
    if let Some(url) = affiliation_url
        && let Some(first_ref) = author.affiliations.first()
        && let Some(aff) = affiliations.iter_mut().find(|a| &a.id == first_ref)
        && aff.url.is_none()
    {
        aff.url = Some(url);
    }

    if author.name.literal.is_empty() {
        return None;
    }
    Some(author)
}

fn set_attribute(author: &mut Author, flag: &str) {
    if !author.attributes.iter().any(|a| a == flag) {
        author.attributes.push(flag.to_string());
    }
}

/// `attributes:` — a list of flag names, or a map of truthy flags.
fn parse_attributes(author: &mut Author, value: &ConfigValue) {
    if let Some(arr) = value.as_array() {
        for item in arr {
            if let Some(flag) = item.as_plain_text() {
                set_attribute(author, &flag);
            }
        }
    } else if let Some(entries) = value.as_map_entries() {
        for entry in entries {
            let truthy = entry.value.as_bool().unwrap_or(false)
                || entry
                    .value
                    .as_plain_text()
                    .is_some_and(|t| t.eq_ignore_ascii_case("true"));
            if truthy {
                set_attribute(author, &entry.key);
            }
        }
    } else if let Some(flag) = value.as_plain_text() {
        set_attribute(author, &flag);
    }
}

/// CRediT contributor-role vocabulary.
const CREDIT_VOCAB_IDENTIFIER: &str = "https://credit.niso.org";

const CREDIT_ROLES: &[(&str, &str)] = &[
    (
        "conceptualization",
        "https://credit.niso.org/contributor-roles/conceptualization/",
    ),
    (
        "data curation",
        "https://credit.niso.org/contributor-roles/data-curation/",
    ),
    (
        "formal analysis",
        "https://credit.niso.org/contributor-roles/formal-analysis/",
    ),
    (
        "funding acquisition",
        "https://credit.niso.org/contributor-roles/funding-acquisition/",
    ),
    (
        "investigation",
        "https://credit.niso.org/contributor-roles/investigation/",
    ),
    (
        "methodology",
        "https://credit.niso.org/contributor-roles/methodology/",
    ),
    (
        "project administration",
        "https://credit.niso.org/contributor-roles/project-administration/",
    ),
    (
        "resources",
        "https://credit.niso.org/contributor-roles/resources/",
    ),
    (
        "software",
        "https://credit.niso.org/contributor-roles/software/",
    ),
    (
        "supervision",
        "https://credit.niso.org/contributor-roles/supervision/",
    ),
    (
        "validation",
        "https://credit.niso.org/contributor-roles/validation/",
    ),
    (
        "visualization",
        "https://credit.niso.org/contributor-roles/visualization/",
    ),
    (
        "writing – original draft",
        "https://credit.niso.org/contributor-roles/writing-original-draft/",
    ),
    (
        "writing – review & editing",
        "https://credit.niso.org/contributor-roles/writing-review-editing/",
    ),
];

const CREDIT_ALIASES: &[(&str, &str)] = &[
    ("writing", "writing – original draft"),
    ("analysis", "formal analysis"),
    ("funding", "funding acquisition"),
    ("editing", "writing – review & editing"),
];

fn set_role(author: &mut Author, role: String, contribution: Option<String>) {
    let mut r = AuthorRole {
        role,
        degree_of_contribution: contribution,
        ..Default::default()
    };
    let mut term = r.role.to_lowercase();
    if let Some((_, canonical)) = CREDIT_ALIASES.iter().find(|(alias, _)| *alias == term) {
        term = canonical.to_string();
    }
    if let Some((canonical, url)) = CREDIT_ROLES.iter().find(|(t, _)| *t == term) {
        r.vocab_identifier = Some(CREDIT_VOCAB_IDENTIFIER.to_string());
        r.vocab_term = Some(canonical.to_string());
        r.vocab_term_identifier = Some(url.to_string());
    }
    author.roles.push(r);
}

/// `role:`/`roles:` — a string, a list of strings/maps, or a map of
/// `role: degree-of-contribution` pairs. A map carrying a `role` key
/// is the normalized (round-trip) object form.
fn parse_roles(author: &mut Author, value: &ConfigValue) {
    if let Some(text) = value.as_plain_text() {
        set_role(author, text, None);
    } else if let Some(arr) = value.as_array() {
        for item in arr {
            parse_roles(author, item);
        }
    } else if let Some(entries) = value.as_map_entries() {
        if let Some(role) = value.get("role").and_then(|r| r.as_plain_text()) {
            let contribution = value
                .get("degree-of-contribution")
                .and_then(|c| c.as_plain_text());
            set_role(author, role, contribution);
        } else {
            for entry in entries {
                if let Some(contribution) = entry.value.as_plain_text() {
                    set_role(author, entry.key.clone(), Some(contribution));
                }
            }
        }
    }
}

/// Affiliation aliased fields: `state` → `region`,
/// `affiliation-url` → `url`.
fn affiliation_field_slot<'a>(
    aff: &'a mut Affiliation,
    key: &str,
) -> Option<&'a mut Option<String>> {
    match key {
        "name" => Some(&mut aff.name),
        "department" => Some(&mut aff.department),
        "group" => Some(&mut aff.group),
        "address" => Some(&mut aff.address),
        "city" => Some(&mut aff.city),
        "region" | "state" => Some(&mut aff.region),
        "country" => Some(&mut aff.country),
        "postal-code" => Some(&mut aff.postal_code),
        "url" | "affiliation-url" => Some(&mut aff.url),
        "isni" => Some(&mut aff.isni),
        "ringgold" => Some(&mut aff.ringgold),
        "ror" => Some(&mut aff.ror),
        _ => None,
    }
}

/// Normalize one affiliation map (or bare string) into a typed
/// [`Affiliation`]. `id` stays empty when unspecified; assignment
/// happens in [`maybe_add_affiliation`].
fn parse_affiliation_obj(value: &ConfigValue) -> Option<Affiliation> {
    let mut aff = Affiliation::default();
    if let Some(text) = value.as_plain_text() {
        aff.name = Some(text);
        return Some(aff);
    }
    let entries = value.as_map_entries()?;
    for entry in entries {
        let key = entry.key.as_str();
        if key == "id" {
            aff.id = entry.value.as_plain_text().unwrap_or_default();
        } else if let Some(slot) = affiliation_field_slot(&mut aff, key) {
            *slot = entry.value.as_plain_text();
        } else {
            aff.metadata.push(entry.clone());
        }
    }
    Some(aff)
}

/// Parse an author's `affiliation:`/`affiliations:` value. `ref:`-only
/// entries become refs on the author directly; everything else is
/// returned for collection into the model.
fn parse_author_affiliations(value: &ConfigValue, author: &mut Author) -> Vec<Affiliation> {
    let mut out = Vec::new();
    let items: Vec<&ConfigValue> = if let Some(arr) = value.as_array() {
        arr.iter().collect()
    } else {
        vec![value]
    };
    for item in items {
        if let Some(entries) = item.as_map_entries()
            && entries.len() == 1
            && entries[0].key == "ref"
        {
            if let Some(r) = entries[0].value.as_plain_text() {
                author.affiliations.push(r);
            }
            continue;
        }
        if let Some(aff) = parse_affiliation_obj(item) {
            out.push(aff);
        }
    }
    out
}

/// Parse a standalone affiliations value (the top-level block).
fn parse_affiliation_values(value: &ConfigValue) -> Vec<Affiliation> {
    let items: Vec<&ConfigValue> = if let Some(arr) = value.as_array() {
        arr.iter().collect()
    } else {
        vec![value]
    };
    items
        .iter()
        .filter_map(|v| parse_affiliation_obj(v))
        .collect()
}

/// True when two affiliations match on every field except `id`
/// (Q1's `findMatchingAffililation`).
fn affiliations_match(a: &Affiliation, b: &Affiliation) -> bool {
    a.name == b.name
        && a.department == b.department
        && a.group == b.group
        && a.address == b.address
        && a.city == b.city
        && a.region == b.region
        && a.country == b.country
        && a.postal_code == b.postal_code
        && a.url == b.url
        && a.isni == b.isni
        && a.ringgold == b.ringgold
        && a.ror == b.ror
}

/// Add `aff` to the list unless an identical affiliation is already
/// there; returns the id to reference it by. Unset ids become `aff-N`.
fn maybe_add_affiliation(mut aff: Affiliation, affiliations: &mut Vec<Affiliation>) -> String {
    if let Some(existing) = affiliations.iter().find(|a| affiliations_match(a, &aff)) {
        return existing.id.clone();
    }
    if aff.id.is_empty() {
        aff.id = format!("aff-{}", affiliations.len() + 1);
    }
    let id = aff.id.clone();
    affiliations.push(aff);
    id
}

// ─────────────────────────────────────────────────────────────────────
// Names
// ─────────────────────────────────────────────────────────────────────

/// Convert a `name:` value (scalar or component map) to a normalized
/// [`AuthorName`].
fn to_name(value: &ConfigValue) -> AuthorName {
    if let Some(text) = value.as_plain_text() {
        return to_name_from_literal(&text);
    }
    let mut name = AuthorName::default();
    if value.as_map_entries().is_some() {
        name.literal = value
            .get("literal")
            .and_then(|v| v.as_plain_text())
            .unwrap_or_default();
        name.given = value.get("given").and_then(|v| v.as_plain_text());
        name.family = value.get("family").and_then(|v| v.as_plain_text());
        name.dropping_particle = value
            .get("dropping-particle")
            .and_then(|v| v.as_plain_text());
        name.non_dropping_particle = value
            .get("non-dropping-particle")
            .and_then(|v| v.as_plain_text());
    }
    normalize_name(name)
}

/// Build a name from a literal display string.
fn to_name_from_literal(literal: &str) -> AuthorName {
    normalize_name(AuthorName {
        literal: literal.trim().to_string(),
        ..Default::default()
    })
}

/// Fill in whichever of literal / components is missing.
fn normalize_name(mut name: AuthorName) -> AuthorName {
    if name.literal.is_empty() {
        name.literal = compose_literal(&name);
    }
    if (name.family.is_none() || name.given.is_none()) && !name.literal.is_empty() {
        let parsed = split_literal_name(&name.literal);
        if name.given.is_none() {
            name.given = parsed.given;
        }
        if name.family.is_none() {
            name.family = parsed.family;
        }
        if name.non_dropping_particle.is_none() {
            name.non_dropping_particle = parsed.non_dropping_particle;
        }
    }
    name
}

/// Compose a display literal from components, in CSL display order:
/// `given [dropping-particle] [non-dropping-particle] family`.
fn compose_literal(name: &AuthorName) -> String {
    let parts: Vec<&str> = [
        name.given.as_deref(),
        name.dropping_particle.as_deref(),
        name.non_dropping_particle.as_deref(),
        name.family.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|p| !p.is_empty())
    .collect();
    parts.join(" ")
}

/// Split a literal display name into given / particle / family using
/// the BibTeX "First von Last" / "von Last, First" heuristic (see the
/// module docs for the deviation note vs. Q1's BibTeX round-trip).
fn split_literal_name(literal: &str) -> AuthorName {
    let mut out = AuthorName::default();

    // "von Last, First" comma form.
    if let Some((before, after)) = literal.split_once(',') {
        let family_part = before.trim();
        out.given = non_empty(after.trim());
        let tokens: Vec<&str> = family_part.split_whitespace().collect();
        let first_upper = tokens
            .iter()
            .position(|t| !starts_lowercase(t))
            .unwrap_or(tokens.len().saturating_sub(1));
        if first_upper > 0 {
            out.non_dropping_particle = non_empty(&tokens[..first_upper].join(" "));
        }
        out.family = non_empty(&tokens[first_upper.min(tokens.len())..].join(" "));
        return out;
    }

    let tokens: Vec<&str> = literal.split_whitespace().collect();
    match tokens.len() {
        0 => {}
        1 => out.family = Some(tokens[0].to_string()),
        n => {
            // Particle span: first lowercase token through the last
            // lowercase token before the final token.
            let first_lower = tokens[..n - 1].iter().position(|t| starts_lowercase(t));
            if let Some(start) = first_lower {
                let end = tokens[..n - 1]
                    .iter()
                    .rposition(|t| starts_lowercase(t))
                    .unwrap_or(start);
                if start > 0 {
                    out.given = Some(tokens[..start].join(" "));
                }
                out.non_dropping_particle = Some(tokens[start..=end].join(" "));
                out.family = Some(tokens[end + 1..].join(" "));
            } else {
                out.given = Some(tokens[..n - 1].join(" "));
                out.family = Some(tokens[n - 1].to_string());
            }
        }
    }
    out
}

fn starts_lowercase(token: &str) -> bool {
    token
        .chars()
        .find(|c| c.is_alphabetic())
        .is_some_and(|c| c.is_lowercase())
}

fn non_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────
// Funding
// ─────────────────────────────────────────────────────────────────────

/// Normalize one funding group. `ref:` values resolve against the
/// already-normalized authors/affiliations (which is why funding is
/// parsed last).
fn parse_funding_group(
    value: &ConfigValue,
    authors: &[Author],
    affiliations: &[Affiliation],
    issues: &mut Vec<String>,
) -> FundingGroup {
    let mut group = FundingGroup::default();
    if value.as_map_entries().is_none() {
        // A bare string/inlines is the statement.
        group.statement = value.as_plain_text();
        return group;
    }
    group.statement = value.get("statement").and_then(|v| v.as_plain_text());
    group.open_access = value.get("open-access").and_then(|v| v.as_plain_text());
    if let Some(awards_raw) = value.get("awards") {
        let items: Vec<&ConfigValue> = if let Some(arr) = awards_raw.as_array() {
            arr.iter().collect()
        } else {
            vec![awards_raw]
        };
        for item in items {
            group
                .awards
                .push(parse_award(item, authors, affiliations, issues));
        }
    }
    group
}

fn parse_award(
    value: &ConfigValue,
    authors: &[Author],
    affiliations: &[Affiliation],
    issues: &mut Vec<String>,
) -> Award {
    let mut award = Award {
        id: value.get("id").and_then(|v| v.as_plain_text()),
        name: value.get("name").and_then(|v| v.as_plain_text()),
        description: value.get("description").and_then(|v| v.as_plain_text()),
        ..Default::default()
    };
    if let Some(source) = value.get("source") {
        for entry in party_entries(source) {
            if let Some(party) = parse_funding_party(entry, authors, affiliations, issues) {
                award.source.push(FundingSource {
                    party,
                    country: entry.get("country").and_then(|v| v.as_plain_text()),
                    source_type: entry.get("type").and_then(|v| v.as_plain_text()),
                });
            }
        }
    }
    if let Some(recipient) = value.get("recipient") {
        award.recipient = party_entries(recipient)
            .into_iter()
            .filter_map(|e| parse_funding_party(e, authors, affiliations, issues))
            .collect();
    }
    if let Some(investigator) = value.get("investigator") {
        award.investigator = party_entries(investigator)
            .into_iter()
            .filter_map(|e| parse_funding_party(e, authors, affiliations, issues))
            .collect();
    }
    award
}

fn party_entries(value: &ConfigValue) -> Vec<&ConfigValue> {
    if let Some(arr) = value.as_array() {
        arr.iter().collect()
    } else {
        vec![value]
    }
}

fn parse_funding_party(
    value: &ConfigValue,
    authors: &[Author],
    affiliations: &[Affiliation],
    issues: &mut Vec<String>,
) -> Option<FundingParty> {
    if let Some(text) = value.as_plain_text() {
        return Some(FundingParty::Text(text));
    }
    if let Some(name) = value.get("name") {
        return Some(FundingParty::Name(to_name(name)));
    }
    if let Some(inst) = value.get("institution") {
        return parse_affiliation_obj(inst).map(|a| FundingParty::Institution(Box::new(a)));
    }
    if let Some(r) = value.get("ref").and_then(|v| v.as_plain_text()) {
        if let Some(aff) = affiliations.iter().find(|a| a.id == r) {
            return Some(FundingParty::Institution(Box::new(aff.clone())));
        }
        if let Some(author) = authors.iter().find(|a| a.id.as_deref() == Some(r.as_str())) {
            return Some(FundingParty::Name(author.name.clone()));
        }
        issues.push(format!("Invalid funding ref '{r}'."));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::{FileId, Location, Range, SourceInfo};

    fn si() -> SourceInfo {
        SourceInfo::from_range(
            FileId(0),
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
            },
        )
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: si(),
                    value: v,
                })
                .collect(),
            si(),
        )
    }

    fn s(text: &str) -> ConfigValue {
        ConfigValue::new_string(text, si())
    }

    fn arr(items: Vec<ConfigValue>) -> ConfigValue {
        ConfigValue::new_array(items, si())
    }

    fn names(meta: &ConfigValue) -> Vec<String> {
        parse_authors(meta)
            .into_iter()
            .map(|a| a.name.literal)
            .collect()
    }

    // ── P1 surface (regression) ──────────────────────────────────────

    #[test]
    fn scalar_author() {
        let meta = map(vec![("author", s("Norah Jones"))]);
        assert_eq!(names(&meta), vec!["Norah Jones"]);
    }

    #[test]
    fn string_list_authors() {
        let meta = map(vec![(
            "author",
            arr(vec![s("Amelia Earhart"), s("Bill Malone")]),
        )]);
        assert_eq!(names(&meta), vec!["Amelia Earhart", "Bill Malone"]);
    }

    #[test]
    fn authors_key_accepted() {
        let meta = map(vec![("authors", arr(vec![s("Norah Jones")]))]);
        assert_eq!(names(&meta), vec!["Norah Jones"]);
    }

    #[test]
    fn map_authors_with_scalar_name() {
        let meta = map(vec![(
            "author",
            arr(vec![
                map(vec![("name", s("Norah Jones")), ("orcid", s("0000"))]),
                map(vec![("name", s("Bill Malone"))]),
            ]),
        )]);
        assert_eq!(names(&meta), vec!["Norah Jones", "Bill Malone"]);
    }

    #[test]
    fn single_map_author() {
        let meta = map(vec![("author", map(vec![("name", s("Norah Jones"))]))]);
        assert_eq!(names(&meta), vec!["Norah Jones"]);
    }

    #[test]
    fn structured_name_components() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![(
                "name",
                map(vec![
                    ("given", s("Vincent")),
                    ("family", s("Gogh")),
                    ("non-dropping-particle", s("van")),
                ]),
            )])]),
        )]);
        assert_eq!(names(&meta), vec!["Vincent van Gogh"]);
    }

    #[test]
    fn structured_name_literal_wins() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![(
                "name",
                map(vec![("literal", s("N. Jones")), ("given", s("Norah"))]),
            )])]),
        )]);
        assert_eq!(names(&meta), vec!["N. Jones"]);
    }

    #[test]
    fn nameless_entry_dropped() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![("orcid", s("0000"))]), s("Bill Malone")]),
        )]);
        assert_eq!(names(&meta), vec!["Bill Malone"]);
    }

    #[test]
    fn no_author_key() {
        let meta = map(vec![("title", s("Untitled"))]);
        assert!(names(&meta).is_empty());
    }

    #[test]
    fn corresponding_bool_does_not_leak() {
        // The bd-8v34zny5 shape: boolean attribute values must never
        // surface as author names.
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![
                ("name", s("Norah Jones")),
                ("corresponding", ConfigValue::new_bool(true, si())),
            ])]),
        )]);
        assert_eq!(names(&meta), vec!["Norah Jones"]);
    }

    // ── P2: names ────────────────────────────────────────────────────

    #[test]
    fn literal_splits_into_given_family() {
        let authors = parse_authors(&map(vec![("author", s("Norah Marie Jones"))]));
        let name = &authors[0].name;
        assert_eq!(name.given.as_deref(), Some("Norah Marie"));
        assert_eq!(name.family.as_deref(), Some("Jones"));
    }

    #[test]
    fn literal_particle_detected() {
        let authors = parse_authors(&map(vec![("author", s("Vincent van Gogh"))]));
        let name = &authors[0].name;
        assert_eq!(name.given.as_deref(), Some("Vincent"));
        assert_eq!(name.non_dropping_particle.as_deref(), Some("van"));
        assert_eq!(name.family.as_deref(), Some("Gogh"));
    }

    #[test]
    fn literal_comma_form() {
        let authors = parse_authors(&map(vec![("author", s("van Gogh, Vincent"))]));
        let name = &authors[0].name;
        assert_eq!(name.given.as_deref(), Some("Vincent"));
        assert_eq!(name.non_dropping_particle.as_deref(), Some("van"));
        assert_eq!(name.family.as_deref(), Some("Gogh"));
        // Literal keeps the authored display form.
        assert_eq!(name.literal, "van Gogh, Vincent");
    }

    #[test]
    fn single_token_name_is_family() {
        let authors = parse_authors(&map(vec![("author", s("Aristotle"))]));
        assert_eq!(authors[0].name.family.as_deref(), Some("Aristotle"));
        assert_eq!(authors[0].name.given, None);
    }

    #[test]
    fn top_level_name_components() {
        // Q1 accepts given/family directly under the author.
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![
                ("given", s("Norah")),
                ("family", s("Jones")),
                ("orcid", s("0000-0002-1825-0097")),
            ])]),
        )]);
        let authors = parse_authors(&meta);
        assert_eq!(authors[0].name.literal, "Norah Jones");
        assert_eq!(authors[0].orcid.as_deref(), Some("0000-0002-1825-0097"));
    }

    // ── P2: simple fields, degrees, attributes, roles, notes ────────

    fn rich_author_meta() -> ConfigValue {
        map(vec![(
            "author",
            arr(vec![
                map(vec![
                    ("name", s("Norah Jones")),
                    ("orcid", s("0000-0002-1825-0097")),
                    ("email", s("norah@example.com")),
                    ("url", s("https://example.com/norah")),
                    ("corresponding", ConfigValue::new_bool(true, si())),
                    ("degrees", arr(vec![s("PhD")])),
                    (
                        "affiliations",
                        arr(vec![map(vec![
                            ("name", s("Carnegie Mellon University")),
                            ("department", s("School of Music")),
                        ])]),
                    ),
                ]),
                map(vec![
                    ("name", s("Bill Malone")),
                    (
                        "affiliations",
                        arr(vec![map(vec![("name", s("University of Texas"))])]),
                    ),
                ]),
            ]),
        )])
    }

    #[test]
    fn simple_fields_and_degrees() {
        let model = parse_authors_model(&rich_author_meta());
        let norah = &model.authors[0];
        assert_eq!(norah.orcid.as_deref(), Some("0000-0002-1825-0097"));
        assert_eq!(norah.email.as_deref(), Some("norah@example.com"));
        assert_eq!(norah.url.as_deref(), Some("https://example.com/norah"));
        assert_eq!(norah.degrees, vec!["PhD"]);
        assert_eq!(norah.attributes, vec!["corresponding"]);
        assert_eq!(norah.number, 1);
        assert_eq!(norah.letter, "a");
        assert_eq!(model.authors[1].number, 2);
        assert_eq!(model.authors[1].letter, "b");
    }

    #[test]
    fn affiliations_collected_and_referenced() {
        let model = parse_authors_model(&rich_author_meta());
        assert_eq!(model.affiliations.len(), 2);
        assert_eq!(model.affiliations[0].id, "aff-1");
        assert_eq!(
            model.affiliations[0].name.as_deref(),
            Some("Carnegie Mellon University")
        );
        assert_eq!(
            model.affiliations[0].department.as_deref(),
            Some("School of Music")
        );
        assert_eq!(model.authors[0].affiliations, vec!["aff-1"]);
        assert_eq!(model.authors[1].affiliations, vec!["aff-2"]);
    }

    #[test]
    fn attributes_list_and_flags() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![
                ("name", s("Norah Jones")),
                ("deceased", ConfigValue::new_bool(true, si())),
                ("attributes", arr(vec![s("custom-flag")])),
            ])]),
        )]);
        let model = parse_authors_model(&meta);
        assert_eq!(model.authors[0].attributes, vec!["deceased", "custom-flag"]);
    }

    #[test]
    fn false_attribute_flag_not_set() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![
                ("name", s("Norah Jones")),
                ("corresponding", ConfigValue::new_bool(false, si())),
            ])]),
        )]);
        let model = parse_authors_model(&meta);
        assert!(model.authors[0].attributes.is_empty());
    }

    #[test]
    fn roles_string_list_and_credit_aliases() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![
                ("name", s("Norah Jones")),
                ("roles", arr(vec![s("writing"), s("conceptualization")])),
            ])]),
        )]);
        let model = parse_authors_model(&meta);
        let roles = &model.authors[0].roles;
        assert_eq!(roles.len(), 2);
        assert_eq!(roles[0].role, "writing");
        assert_eq!(
            roles[0].vocab_term.as_deref(),
            Some("writing – original draft")
        );
        assert_eq!(
            roles[0].vocab_identifier.as_deref(),
            Some("https://credit.niso.org")
        );
        assert_eq!(roles[1].vocab_term.as_deref(), Some("conceptualization"));
    }

    #[test]
    fn roles_map_with_contribution() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![
                ("name", s("Norah Jones")),
                ("roles", arr(vec![map(vec![("software", s("lead"))])])),
            ])]),
        )]);
        let model = parse_authors_model(&meta);
        let role = &model.authors[0].roles[0];
        assert_eq!(role.role, "software");
        assert_eq!(role.degree_of_contribution.as_deref(), Some("lead"));
        assert_eq!(role.vocab_term.as_deref(), Some("software"));
    }

    #[test]
    fn non_credit_role_gets_no_vocab() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![
                ("name", s("Norah Jones")),
                ("role", s("chief visionary")),
            ])]),
        )]);
        let model = parse_authors_model(&meta);
        let role = &model.authors[0].roles[0];
        assert_eq!(role.role, "chief visionary");
        assert!(role.vocab_term.is_none());
    }

    #[test]
    fn notes_numbered_across_authors() {
        let meta = map(vec![(
            "author",
            arr(vec![
                map(vec![("name", s("A B")), ("note", s("First note"))]),
                map(vec![("name", s("C D"))]),
                map(vec![("name", s("E F")), ("note", s("Second note"))]),
            ]),
        )]);
        let model = parse_authors_model(&meta);
        assert_eq!(model.authors[0].note.as_ref().unwrap().number, 1);
        assert_eq!(model.authors[0].note.as_ref().unwrap().text, "First note");
        assert!(model.authors[1].note.is_none());
        assert_eq!(model.authors[2].note.as_ref().unwrap().number, 2);
    }

    #[test]
    fn unknown_keys_bucketed_as_metadata() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![
                ("name", s("Norah Jones")),
                ("favorite-color", s("blue")),
            ])]),
        )]);
        let model = parse_authors_model(&meta);
        assert_eq!(model.authors[0].metadata.len(), 1);
        assert_eq!(model.authors[0].metadata[0].key, "favorite-color");
    }

    // ── P2: affiliations ─────────────────────────────────────────────

    #[test]
    fn identical_affiliations_dedup() {
        let meta = map(vec![(
            "author",
            arr(vec![
                map(vec![
                    ("name", s("A B")),
                    ("affiliations", arr(vec![s("Same University")])),
                ]),
                map(vec![
                    ("name", s("C D")),
                    ("affiliations", arr(vec![s("Same University")])),
                ]),
            ]),
        )]);
        let model = parse_authors_model(&meta);
        assert_eq!(model.affiliations.len(), 1);
        assert_eq!(model.authors[0].affiliations, vec!["aff-1"]);
        assert_eq!(model.authors[1].affiliations, vec!["aff-1"]);
    }

    #[test]
    fn string_affiliation_is_name() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![
                ("name", s("A B")),
                ("affiliation", s("Some University")),
            ])]),
        )]);
        let model = parse_authors_model(&meta);
        assert_eq!(
            model.affiliations[0].name.as_deref(),
            Some("Some University")
        );
    }

    #[test]
    fn state_aliases_to_region() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![
                ("name", s("A B")),
                (
                    "affiliations",
                    arr(vec![map(vec![("name", s("UT")), ("state", s("Texas"))])]),
                ),
            ])]),
        )]);
        let model = parse_authors_model(&meta);
        assert_eq!(model.affiliations[0].region.as_deref(), Some("Texas"));
    }

    #[test]
    fn ref_resolves_against_top_level_block() {
        let meta = map(vec![
            (
                "author",
                arr(vec![map(vec![
                    ("name", s("A B")),
                    ("affiliations", arr(vec![map(vec![("ref", s("cmu"))])])),
                ])]),
            ),
            (
                "affiliations",
                arr(vec![map(vec![
                    ("id", s("cmu")),
                    ("name", s("Carnegie Mellon University")),
                ])]),
            ),
        ]);
        let model = parse_authors_model(&meta);
        assert_eq!(model.affiliations.len(), 1);
        assert_eq!(model.affiliations[0].id, "cmu");
        assert_eq!(model.authors[0].affiliations, vec!["cmu"]);
        assert!(model.issues.is_empty());
    }

    #[test]
    fn top_level_duplicate_remaps_refs() {
        // An author defines the affiliation inline; the top-level
        // block re-defines the identical affiliation with an explicit
        // id, and a second author refs that id. The two merge, and
        // the ref remaps onto the survivor.
        let meta = map(vec![
            (
                "author",
                arr(vec![
                    map(vec![
                        ("name", s("A B")),
                        (
                            "affiliations",
                            arr(vec![map(vec![("name", s("Same University"))])]),
                        ),
                    ]),
                    map(vec![
                        ("name", s("C D")),
                        ("affiliations", arr(vec![map(vec![("ref", s("same-u"))])])),
                    ]),
                ]),
            ),
            (
                "affiliations",
                arr(vec![map(vec![
                    ("id", s("same-u")),
                    ("name", s("Same University")),
                ])]),
            ),
        ]);
        let model = parse_authors_model(&meta);
        assert_eq!(model.affiliations.len(), 1);
        assert_eq!(model.affiliations[0].id, "aff-1");
        assert_eq!(model.authors[1].affiliations, vec!["aff-1"]);
        assert!(model.issues.is_empty());
    }

    #[test]
    fn undefined_ref_dropped_with_issue() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![
                ("name", s("A B")),
                ("affiliations", arr(vec![map(vec![("ref", s("nowhere"))])])),
            ])]),
        )]);
        let model = parse_authors_model(&meta);
        assert!(model.authors[0].affiliations.is_empty());
        assert_eq!(model.issues.len(), 1);
        assert!(model.issues[0].contains("nowhere"));
        assert!(model.issues[0].contains("A B"));
    }

    #[test]
    fn affiliation_url_decorates_first_affiliation() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![
                ("name", s("A B")),
                ("affiliation", s("Some University")),
                ("affiliation-url", s("https://some.edu")),
            ])]),
        )]);
        let model = parse_authors_model(&meta);
        assert_eq!(
            model.affiliations[0].url.as_deref(),
            Some("https://some.edu")
        );
    }

    #[test]
    fn affiliation_unknown_keys_bucketed() {
        let meta = map(vec![(
            "author",
            arr(vec![map(vec![
                ("name", s("A B")),
                (
                    "affiliations",
                    arr(vec![map(vec![("name", s("U")), ("mascot", s("owl"))])]),
                ),
            ])]),
        )]);
        let model = parse_authors_model(&meta);
        assert_eq!(model.affiliations[0].metadata.len(), 1);
        assert_eq!(model.affiliations[0].metadata[0].key, "mascot");
    }

    #[test]
    fn affiliation_letters_assigned() {
        let model = parse_authors_model(&rich_author_meta());
        assert_eq!(model.affiliations[0].number, 1);
        assert_eq!(model.affiliations[0].letter, "a");
        assert_eq!(model.affiliations[1].letter, "b");
    }

    #[test]
    fn letters_go_past_z() {
        assert_eq!(letter(1), "a");
        assert_eq!(letter(26), "z");
        assert_eq!(letter(27), "aa");
        assert_eq!(letter(28), "ab");
    }

    // ── P2: funding ──────────────────────────────────────────────────

    #[test]
    fn funding_bare_string_is_statement() {
        let meta = map(vec![
            ("author", s("A B")),
            ("funding", s("Funded by the Example Foundation.")),
        ]);
        let model = parse_authors_model(&meta);
        assert_eq!(model.funding.len(), 1);
        assert_eq!(
            model.funding[0].statement.as_deref(),
            Some("Funded by the Example Foundation.")
        );
    }

    #[test]
    fn funding_award_with_refs() {
        let meta = map(vec![
            (
                "author",
                arr(vec![map(vec![
                    ("id", s("nj")),
                    ("name", s("Norah Jones")),
                    ("affiliations", arr(vec![map(vec![("ref", s("cmu"))])])),
                ])]),
            ),
            (
                "affiliations",
                arr(vec![map(vec![
                    ("id", s("cmu")),
                    ("name", s("Carnegie Mellon University")),
                ])]),
            ),
            (
                "funding",
                arr(vec![map(vec![
                    ("statement", s("Grant 42")),
                    (
                        "awards",
                        arr(vec![map(vec![
                            ("id", s("42")),
                            (
                                "source",
                                arr(vec![map(vec![
                                    ("institution", map(vec![("name", s("NSF"))])),
                                    ("country", s("US")),
                                ])]),
                            ),
                            ("recipient", arr(vec![map(vec![("ref", s("nj"))])])),
                            ("investigator", arr(vec![map(vec![("ref", s("cmu"))])])),
                        ])]),
                    ),
                ])]),
            ),
        ]);
        let model = parse_authors_model(&meta);
        assert_eq!(model.funding.len(), 1);
        let award = &model.funding[0].awards[0];
        assert_eq!(award.id.as_deref(), Some("42"));
        match &award.source[0].party {
            FundingParty::Institution(a) => assert_eq!(a.name.as_deref(), Some("NSF")),
            other => panic!("expected institution, got {other:?}"),
        }
        assert_eq!(award.source[0].country.as_deref(), Some("US"));
        match &award.recipient[0] {
            FundingParty::Name(n) => assert_eq!(n.literal, "Norah Jones"),
            other => panic!("expected name, got {other:?}"),
        }
        match &award.investigator[0] {
            FundingParty::Institution(a) => {
                assert_eq!(a.name.as_deref(), Some("Carnegie Mellon University"))
            }
            other => panic!("expected institution, got {other:?}"),
        }
    }

    #[test]
    fn funding_bad_ref_reported() {
        let meta = map(vec![
            ("author", s("A B")),
            (
                "funding",
                map(vec![(
                    "awards",
                    arr(vec![map(vec![(
                        "recipient",
                        arr(vec![map(vec![("ref", s("ghost"))])]),
                    )])]),
                )]),
            ),
        ]);
        let model = parse_authors_model(&meta);
        assert!(model.funding[0].awards[0].recipient.is_empty());
        assert!(model.issues.iter().any(|i| i.contains("ghost")));
    }
}
