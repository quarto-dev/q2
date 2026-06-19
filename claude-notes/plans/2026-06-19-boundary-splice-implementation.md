# Boundary-addressed splice — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generalize the replace-1 `apply_node_edit` into one boundary-addressed splice primitive that also expresses insert (0→N), sibling range (M→N), and delete — driven by a small EDSL of pure verb-builders on the client.

**Architecture:** Every edit lowers to `splice(parent_blocks, from..to, replacement)` over one container's child `Blocks` slice. `from`/`to` are *boundaries* (gaps): `beforeNode(si)` / `afterNode(si)` resolve a block by exact `SourceInfo` match; `startOf`/`endOf` of a `ContainerRef` (`DocRoot | Node(si)`) resolve to gap `0` / `len`. The backend builds `A_u'` and reuses the existing `compute_reconciliation` + `incremental_write` (diff-driven; no writer changes). The client EDSL is pure descriptor-builders; markdown→AST normalization stays at the parent edge.

**Tech Stack:** Rust (`pampa`, `wasm-quarto-hub-client`), TypeScript/React (`ts-packages/preview-renderer`, `hub-client`), `wasm-bindgen`.

**Design spec:** `claude-notes/plans/2026-06-18-boundary-splice-edit-design.md` (read it first). Item plane is a **non-goal** here — see `claude-notes/plans/2026-06-19-item-plane-research.md`.

## Global Constraints

- Tests: `cargo nextest run` (never `cargo test`); never pipe nextest through `tail`.
- Rust integration tests live in `crates/<crate>/tests/integration/<name>.rs` + `pub mod` in `main.rs`. Add to the existing `crates/pampa/tests/integration/node_edit_tests.rs`.
- Run `cargo fmt` on every Rust file after editing (a hook also does this).
- After any change under `pampa` / `quarto-core` / types crossing the wasm-bindgen boundary, the preview is **not** refreshed by `cargo build` alone — rebuild WASM: `cd hub-client && npm run build:wasm` then `cargo xtask build-q2-preview-spa`.
- hub-client: `npm install` from repo root only; production check is `cd hub-client && npm run build:all` (stricter than `tsc --noEmit`).
- Full gate before any push: `cargo xtask verify` (or `--skip-hub-build` for Rust-only steps). **Never push without explicit user permission.**
- TDD is non-negotiable: write the failing test, run it, see it fail for the right reason, implement, see it pass.

## File Structure

**Rust (backend)**
- Modify `crates/pampa/src/apply_node_edit.rs` — add `Boundary`/`ContainerRef`/`Splice` (serde) types, boundary resolver, generalized range splice (`splice_in_blocks` gains a `Range`), public `apply_node_splice`; `apply_node_edit` becomes a shim. (Splice helpers `splice_in_blocks`/`preserve_leaf_variant` already live here — keep them local rather than churn a new module.)
- Use `crates/pampa/src/node_lookup.rs` as-is (`lookup_block`, `NodePath`, `ContainerStep`).
- Test `crates/pampa/tests/integration/node_edit_tests.rs` — add splice tests alongside the existing `apply_node_edit_*`.

**Rust (WASM)**
- Modify `crates/wasm-quarto-hub-client/src/lib.rs` — add `apply_node_splice` export next to `apply_node_edit` (~line 2803).

**TypeScript (preview-renderer, inside the iframe)**
- Create `ts-packages/preview-renderer/src/q2-preview/edit.ts` — `Content`/`Boundary`/`ContainerRef`/`Splice` types + `md`/`ast`/`EMPTY` + verb vocabulary (pure).
- Create `ts-packages/preview-renderer/src/q2-preview/edit.test.tsx` — verb-builder unit tests.
- Modify `ts-packages/preview-renderer/src/q2-preview/PreviewContext.tsx` — context exposes `commit`, drops `commitTextEdit`/`commitSubtreeEdit`.
- Modify `ts-packages/preview-renderer/src/q2-preview/PreviewRoot.tsx` — implement `commit` (build `Splice`, call `setAst`), remove the two old commit fns.
- Modify `ts-packages/preview-renderer/src/q2-preview/usePreviewEdit.ts` — return `{ resolveSource, commit }`.
- Modify `ts-packages/preview-renderer/src/framework/dispatchers.tsx` — `EditTextarea` + delete-by-emptying use `commit(replaceNode/ deleteNode)`.

**TypeScript (hub-client, the parent)**
- Modify `hub-client/src/components/render/ReactPreview.tsx` — `handleSetAst` routes the generalized `Splice`, normalizes `Content`, calls `applyNodeSplice`.
- Modify `hub-client/src/types/wasm-quarto-hub-client.d.ts` — declare `apply_node_splice`.

**Demo render components (outside the repo)**
- Modify `~/docs/demo-playground/gordon/render-components2/{drag,kanban,comment}.tsx` — `commitSubtreeEdit(si, m)` → `commit(replaceNode(si, ast(m)))`.

---

## Task 1: Rust — generalize the splice to a gap range

**Files:**
- Modify: `crates/pampa/src/apply_node_edit.rs`
- Test: `crates/pampa/tests/integration/node_edit_tests.rs`

**Interfaces:**
- Consumes: `NodePath { steps: Vec<ContainerStep>, leaf_idx }`, `ContainerStep`, `Blocks`, `Block` (existing).
- Produces: `fn splice_range(root: &mut Blocks, steps: &[ContainerStep], range: Range<usize>, replacement: Vec<Block>)` and `fn navigate<'a>(root: &'a Blocks, steps: &[ContainerStep]) -> Option<&'a Blocks>`.

- [ ] **Step 1: Write the failing test** — insert (empty range) and range-replace at top level, plus the existing replace-1 still works. Add to `node_edit_tests.rs`:

```rust
#[test]
fn splice_range_insert_at_gap_top_level() {
    // [A, B, C]; insert X at gap 2 (between B and C) → [A, B, X, C]
    let mut blocks = vec![para("A"), para("B"), para("C")];
    splice_range(&mut blocks, &[], 2..2, vec![para("X")]);
    assert_eq!(block_texts(&blocks), vec!["A", "B", "X", "C"]);
}

#[test]
fn splice_range_replace_span_top_level() {
    // [A, B, C]; replace gap 0..2 (A,B) with [X] → [X, C]
    let mut blocks = vec![para("A"), para("B"), para("C")];
    splice_range(&mut blocks, &[], 0..2, vec![para("X")]);
    assert_eq!(block_texts(&blocks), vec!["X", "C"]);
}

#[test]
fn splice_range_delete_span_top_level() {
    let mut blocks = vec![para("A"), para("B"), para("C")];
    splice_range(&mut blocks, &[], 1..3, vec![]);
    assert_eq!(block_texts(&blocks), vec!["A"]);
}
```

Add small local test helpers if not already present in the file:

```rust
fn para(text: &str) -> Block {
    use crate::pandoc::block::Paragraph;
    use crate::pandoc::inline::Str;
    Block::Paragraph(Paragraph {
        content: vec![crate::pandoc::Inline::Str(Str {
            text: text.to_string(),
            source_info: quarto_source_map::SourceInfo::Generated {
                by: quarto_source_map::By { kind: "test".into(), data: serde_json::Value::Null },
                children: vec![],
            },
        })],
        source_info: quarto_source_map::SourceInfo::Generated {
            by: quarto_source_map::By { kind: "test".into(), data: serde_json::Value::Null },
            children: vec![],
        },
    })
}
fn block_texts(blocks: &[Block]) -> Vec<String> {
    blocks.iter().map(|b| match b {
        Block::Paragraph(p) | Block::Plain(_) if matches!(b, Block::Paragraph(_)) => {
            if let Block::Paragraph(p) = b { plain_text(&p.content) } else { unreachable!() }
        }
        _ => String::new(),
    }).collect()
}
```

(If `para`/`block_texts`/`plain_text` already exist in this test module, reuse them; do not duplicate.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p pampa -E 'test(splice_range_)'`
Expected: FAIL — `splice_range`/`navigate` not found.

- [ ] **Step 3: Implement `splice_range` + `navigate`** by generalizing the existing `splice_in_blocks`. Replace the `leaf_idx` terminal with a `Range<usize>`:

```rust
use std::ops::Range;

/// Splice `replacement` into the block tree at `steps` over the half-open
/// gap `range`. `range.start == range.end` is an insert; `range.len() == 1`
/// is a single-block replace (where `preserve_leaf_variant` may apply).
pub(crate) fn splice_range(
    root: &mut Blocks,
    steps: &[ContainerStep],
    range: Range<usize>,
    replacement: Vec<Block>,
) {
    splice_in_blocks(root, steps, range, replacement);
}

fn splice_in_blocks(
    current: &mut Blocks,
    steps: &[ContainerStep],
    range: Range<usize>,
    replacement: Vec<Block>,
) {
    let Some((head, tail)) = steps.split_first() else {
        // Single-block replace keeps the Plain↔Para tight-list coercion.
        let replacement = if range.end - range.start == 1 {
            preserve_leaf_variant(current.get(range.start), replacement)
        } else {
            replacement
        };
        current.splice(range, replacement);
        return;
    };
    match head {
        ContainerStep::Blocks(i) => {
            let child = match &mut current[*i] {
                Block::Div(d) => &mut d.content,
                Block::BlockQuote(bq) => &mut bq.content,
                Block::Figure(f) => &mut f.content,
                b => unreachable!("Blocks step at {i} but got {:?}", std::mem::discriminant(b)),
            };
            splice_in_blocks(child, tail, range, replacement);
        }
        ContainerStep::ListItem(i, item) => {
            let child = match &mut current[*i] {
                Block::BulletList(bl) => &mut bl.content[*item],
                Block::OrderedList(ol) => &mut ol.content[*item],
                b => unreachable!("ListItem step at {i} but got {:?}", std::mem::discriminant(b)),
            };
            splice_in_blocks(child, tail, range, replacement);
        }
        ContainerStep::DefBody(i, def, body) => {
            let child = match &mut current[*i] {
                Block::DefinitionList(dl) => &mut dl.content[*def].1[*body],
                b => unreachable!("DefBody step at {i} but got {:?}", std::mem::discriminant(b)),
            };
            splice_in_blocks(child, tail, range, replacement);
        }
    }
}

/// Immutable navigation to the `Blocks` slice at `steps` (for gap-length
/// computation during boundary resolution).
pub(crate) fn navigate<'a>(root: &'a Blocks, steps: &[ContainerStep]) -> Option<&'a Blocks> {
    let Some((head, tail)) = steps.split_first() else { return Some(root) };
    let child = match head {
        ContainerStep::Blocks(i) => match root.get(*i)? {
            Block::Div(d) => &d.content,
            Block::BlockQuote(bq) => &bq.content,
            Block::Figure(f) => &f.content,
            _ => return None,
        },
        ContainerStep::ListItem(i, item) => match root.get(*i)? {
            Block::BulletList(bl) => bl.content.get(*item)?,
            Block::OrderedList(ol) => ol.content.get(*item)?,
            _ => return None,
        },
        ContainerStep::DefBody(i, def, body) => match root.get(*i)? {
            Block::DefinitionList(dl) => dl.content.get(*def)?.1.get(*body)?,
            _ => return None,
        },
    };
    navigate(child, tail)
}
```

Update the existing `splice_at_path` caller (Step 5 of `apply_node_edit`) to call `splice_range(&mut a_u_prime.blocks, &path.steps, path.leaf_idx..path.leaf_idx + 1, subtree.blocks)`; delete the now-unused `splice_at_path`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p pampa -E 'binary(integration) & test(node_edit_tests)'`
Expected: PASS — new `splice_range_*` and all existing `apply_node_edit_*` green (the replace-1 path is now `range = i..i+1`).

- [ ] **Step 5: Commit**

```bash
git add crates/pampa/src/apply_node_edit.rs crates/pampa/tests/integration/node_edit_tests.rs
git commit -m "feat(pampa): generalize block splice to a gap range (splice_range + navigate)"
```

---

## Task 2: Rust — boundary resolver

**Files:**
- Modify: `crates/pampa/src/apply_node_edit.rs`
- Test: `crates/pampa/tests/integration/node_edit_tests.rs`

**Interfaces:**
- Consumes: `lookup_block`, `navigate`, `decode_compact_source_info`, `NodePath`.
- Produces: serde types `Boundary`, `ContainerRef`; `fn resolve_boundary(a_u: &Pandoc, b: &Boundary) -> Option<(Vec<ContainerStep>, usize)>` returning `(steps, gap_idx)`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn resolve_boundary_after_node_top_level() {
    // doc "A\n\nB\n" → blocks A (si 0..1), B (si 3..4) [offsets illustrative]
    let (a_u, _) = read_doc("A\n\nB\n");
    let b_si = compact_si_of(&a_u.blocks[1]); // {"t":0,"r":[..],"d":0}
    let bound: Boundary = serde_json::from_value(serde_json::json!({
        "kind": "afterNode", "si": b_si
    })).unwrap();
    let (steps, gap) = resolve_boundary(&a_u, &bound).unwrap();
    assert!(steps.is_empty());
    assert_eq!(gap, 2); // after block index 1
}

#[test]
fn resolve_boundary_end_of_doc_root() {
    let (a_u, _) = read_doc("A\n\nB\n");
    let bound: Boundary = serde_json::from_value(serde_json::json!({
        "kind": "endOf", "container": { "kind": "docRoot" }
    })).unwrap();
    let (steps, gap) = resolve_boundary(&a_u, &bound).unwrap();
    assert!(steps.is_empty());
    assert_eq!(gap, a_u.blocks.len()); // == 2
}

#[test]
fn resolve_boundary_start_of_empty_div() {
    // a div with no children: ::: {.x}\n:::\n
    let (a_u, _) = read_doc("::: {.x}\n:::\n");
    let div_si = compact_si_of(&a_u.blocks[0]);
    let bound: Boundary = serde_json::from_value(serde_json::json!({
        "kind": "startOf", "container": { "kind": "node", "si": div_si }
    })).unwrap();
    let (steps, gap) = resolve_boundary(&a_u, &bound).unwrap();
    assert_eq!(steps, vec![ContainerStep::Blocks(0)]);
    assert_eq!(gap, 0);
}
```

Add helpers if absent: `read_doc(src) -> (Pandoc, _)` via the json/qmd reader used elsewhere in this test file, and `compact_si_of(&Block) -> serde_json::Value` building `{"t":0,"r":[start,end],"d":file_id}` from `block.source_info()`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo nextest run -p pampa -E 'test(resolve_boundary_)'`
Expected: FAIL — `Boundary`/`resolve_boundary` not found.

- [ ] **Step 3: Implement the types + resolver**

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ContainerRef {
    DocRoot,
    Node { si: serde_json::Value },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Boundary {
    BeforeNode { si: serde_json::Value },
    AfterNode { si: serde_json::Value },
    StartOf { container: ContainerRef },
    EndOf { container: ContainerRef },
}

/// Parse an `si` JSON value (compact `{t,r,d}` or serde-enum) into SourceInfo,
/// reusing the dual-format logic from `apply_node_edit`.
fn decode_si(v: &serde_json::Value) -> Result<SourceInfo, ApplyNodeEditError> {
    if v.get("t").and_then(|t| t.as_u64()).is_some() {
        decode_compact_source_info(v.clone())
    } else {
        serde_json::from_value(v.clone())
            .map_err(|e| ApplyNodeEditError::DeserializeSourceInfo(format!("{e}")))
    }
}

/// Resolve a boundary to `(steps, gap_idx)` against `A_u`. `None` on any miss
/// (caller degrades by returning original content).
fn resolve_boundary(
    a_u: &Pandoc,
    boundary: &Boundary,
) -> Option<(Vec<ContainerStep>, usize)> {
    match boundary {
        Boundary::BeforeNode { si } => {
            let target = decode_si(si).ok()?;
            let path = lookup_block(a_u, &target, FileId(0))?;
            Some((path.steps, path.leaf_idx))
        }
        Boundary::AfterNode { si } => {
            let target = decode_si(si).ok()?;
            let path = lookup_block(a_u, &target, FileId(0))?;
            Some((path.steps, path.leaf_idx + 1))
        }
        Boundary::StartOf { container } => {
            let steps = resolve_container(a_u, container)?;
            Some((steps, 0))
        }
        Boundary::EndOf { container } => {
            let steps = resolve_container(a_u, container)?;
            let len = navigate(&a_u.blocks, &steps)?.len();
            Some((steps, len))
        }
    }
}

/// Resolve a ContainerRef to the `steps` that land on its child `Blocks` slice.
fn resolve_container(a_u: &Pandoc, container: &ContainerRef) -> Option<Vec<ContainerStep>> {
    match container {
        ContainerRef::DocRoot => Some(vec![]),
        ContainerRef::Node { si } => {
            let target = decode_si(si).ok()?;
            let path = lookup_block(a_u, &target, FileId(0))?;
            // Only single-slice containers are valid Node containers.
            let block = navigate(&a_u.blocks, &path.steps)?.get(path.leaf_idx)?;
            match block {
                Block::Div(_) | Block::BlockQuote(_) | Block::Figure(_) => {
                    let mut steps = path.steps;
                    steps.push(ContainerStep::Blocks(path.leaf_idx));
                    Some(steps)
                }
                _ => None, // not a single-slice container → degrade
            }
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo nextest run -p pampa -E 'test(resolve_boundary_)'`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/pampa/src/apply_node_edit.rs crates/pampa/tests/integration/node_edit_tests.rs
git commit -m "feat(pampa): Boundary/ContainerRef types + resolver (DocRoot | Node)"
```

---

## Task 3: Rust — `apply_node_splice` + `apply_node_edit` shim

**Files:**
- Modify: `crates/pampa/src/apply_node_edit.rs`
- Test: `crates/pampa/tests/integration/node_edit_tests.rs`

**Interfaces:**
- Consumes: `resolve_boundary`, `splice_range`, `read_completing_source_info`, `compute_reconciliation`, `incremental_write`.
- Produces: `pub fn apply_node_splice(content: &str, untransformed_ast_json: &str, splice_json: &str) -> Result<String, ApplyNodeEditError>`.

- [ ] **Step 1: Write the failing tests** — end-to-end qmd→qmd for the headline ops + degrade:

```rust
fn splice_json(from: serde_json::Value, to: serde_json::Value, repl_blocks: serde_json::Value) -> String {
    serde_json::json!({
        "from": from, "to": to,
        "replacement": { "pandoc-api-version": [1, 23, 0], "meta": {}, "blocks": repl_blocks }
    }).to_string()
}

#[test]
fn apply_node_splice_insert_after_top_level() {
    let content = "A\n\nB\n";
    let (a_u, _) = read_doc(content);
    let au_json = write_doc_json(&a_u);
    let after_a = serde_json::json!({"kind":"afterNode","si":compact_si_of(&a_u.blocks[0])});
    let repl = serde_json::json!([{"t":"Para","c":[{"t":"Str","c":"X"}]}]);
    let out = apply_node_splice(content, &au_json, &splice_json(after_a.clone(), after_a, repl)).unwrap();
    assert_eq!(out, "A\n\nX\n\nB\n");
}

#[test]
fn apply_node_splice_append_to_doc() {
    let content = "A\n\nB\n";
    let (a_u, _) = read_doc(content);
    let au_json = write_doc_json(&a_u);
    let end = serde_json::json!({"kind":"endOf","container":{"kind":"docRoot"}});
    let repl = serde_json::json!([{"t":"Para","c":[{"t":"Str","c":"C"}]}]);
    let out = apply_node_splice(content, &au_json, &splice_json(end.clone(), end, repl)).unwrap();
    assert_eq!(out, "A\n\nB\n\nC\n");
}

#[test]
fn apply_node_splice_range_replace_two_with_one() {
    let content = "A\n\nB\n\nC\n";
    let (a_u, _) = read_doc(content);
    let au_json = write_doc_json(&a_u);
    let from = serde_json::json!({"kind":"beforeNode","si":compact_si_of(&a_u.blocks[0])});
    let to   = serde_json::json!({"kind":"afterNode","si":compact_si_of(&a_u.blocks[1])});
    let repl = serde_json::json!([{"t":"Para","c":[{"t":"Str","c":"X"}]}]);
    let out = apply_node_splice(content, &au_json, &splice_json(from, to, repl)).unwrap();
    assert_eq!(out, "X\n\nC\n");
}

#[test]
fn apply_node_splice_delete_via_empty_replacement() {
    let content = "A\n\nB\n\nC\n";
    let (a_u, _) = read_doc(content);
    let au_json = write_doc_json(&a_u);
    let from = serde_json::json!({"kind":"beforeNode","si":compact_si_of(&a_u.blocks[1])});
    let to   = serde_json::json!({"kind":"afterNode","si":compact_si_of(&a_u.blocks[1])});
    let out = apply_node_splice(content, &au_json, &splice_json(from, to, serde_json::json!([]))).unwrap();
    assert_eq!(out, "A\n\nC\n");
}

#[test]
fn apply_node_splice_degrades_on_stale_target() {
    let content = "A\n\nB\n";
    let (a_u, _) = read_doc(content);
    let au_json = write_doc_json(&a_u);
    // si that matches nothing
    let bogus = serde_json::json!({"kind":"afterNode","si":{"t":0,"r":[999,1000],"d":0}});
    let repl = serde_json::json!([{"t":"Para","c":[{"t":"Str","c":"X"}]}]);
    let out = apply_node_splice(content, &au_json, &splice_json(bogus.clone(), bogus, repl)).unwrap();
    assert_eq!(out, content); // unchanged
}

#[test]
fn apply_node_splice_degrades_on_cross_container() {
    // first si top-level, second si inside a div → different steps → degrade
    let content = "A\n\n::: {.x}\nB\n:::\n";
    let (a_u, _) = read_doc(content);
    let au_json = write_doc_json(&a_u);
    let from = serde_json::json!({"kind":"beforeNode","si":compact_si_of(&a_u.blocks[0])});
    // B is nested; grab its si via a helper that finds the inner para
    let b_si = compact_si_of_nested_para(&a_u, "B");
    let to = serde_json::json!({"kind":"afterNode","si":b_si});
    let repl = serde_json::json!([{"t":"Para","c":[{"t":"Str","c":"X"}]}]);
    let out = apply_node_splice(content, &au_json, &splice_json(from, to, repl)).unwrap();
    assert_eq!(out, content); // unchanged — cross-container
}
```

(The exact expected separators — `\n\n` between blocks — must be confirmed against the writer in Step 4; if the writer emits a different but valid separator, update the expected strings to what the writer actually produces and note it in the commit. Do **not** loosen the assertion to "contains".)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p pampa -E 'test(apply_node_splice_)'`
Expected: FAIL — `apply_node_splice` not found.

- [ ] **Step 3: Implement `apply_node_splice` and reduce `apply_node_edit` to a shim**

```rust
#[derive(Debug, Deserialize)]
struct SpliceWire {
    from: Boundary,
    to: Boundary,
    replacement: serde_json::Value, // full Pandoc doc; only blocks used
}

pub fn apply_node_splice(
    content: &str,
    untransformed_ast_json: &str,
    splice_json: &str,
) -> Result<String, ApplyNodeEditError> {
    // 1. A_u
    let mut cursor = Cursor::new(untransformed_ast_json.as_bytes());
    let (a_u, _ctx) = json_read(&mut cursor)
        .map_err(|e| ApplyNodeEditError::DeserializeUntransformedAst(format!("{e:?}")))?;

    // 2. Splice descriptor
    let wire: SpliceWire = serde_json::from_str(splice_json)
        .map_err(|e| ApplyNodeEditError::DeserializeModifiedSubtree(format!("{e}")))?;

    // 3. Resolve both boundaries; degrade gracefully on any miss.
    let (Some((from_steps, from_gap)), Some((to_steps, to_gap))) = (
        resolve_boundary(&a_u, &wire.from),
        resolve_boundary(&a_u, &wire.to),
    ) else {
        eprintln!("[apply_node_splice] boundary did not resolve; returning original (stale-AST)");
        return Ok(content.to_string());
    };

    // 4. Invariants: same container, ordered.
    if from_steps != to_steps || from_gap > to_gap {
        eprintln!("[apply_node_splice] cross-container or from>to; returning original");
        return Ok(content.to_string());
    }

    // 5. Replacement blocks (lenient completing read, as apply_node_edit does).
    let repl_json = serde_json::to_string(&wire.replacement)
        .map_err(|e| ApplyNodeEditError::DeserializeModifiedSubtree(format!("{e}")))?;
    let mut cursor = Cursor::new(repl_json.as_bytes());
    let completing_by = By { kind: "direct-write".to_string(), data: serde_json::Value::Null };
    let (subtree, _) = read_completing_source_info(&mut cursor, completing_by)
        .map_err(|e| ApplyNodeEditError::DeserializeModifiedSubtree(format!("{e:?}")))?;

    // 6. Splice → A_u'.
    let mut a_u_prime = a_u.clone();
    splice_range(&mut a_u_prime.blocks, &from_steps, from_gap..to_gap, subtree.blocks);

    // 7. Reconcile + write.
    let plan = compute_reconciliation(&a_u, &a_u_prime);
    incremental_write(content, &a_u, &a_u_prime, &plan)
        .map_err(|e| ApplyNodeEditError::IncrementalWrite(format!("{e:?}")))
}

/// Back-compat shim: replace exactly one block.
pub fn apply_node_edit(
    content: &str,
    untransformed_ast_json: &str,
    destination_source_info_json: &str,
    modified_subtree_json: &str,
) -> Result<String, ApplyNodeEditError> {
    let si: serde_json::Value = serde_json::from_str(destination_source_info_json)
        .map_err(|e| ApplyNodeEditError::DeserializeSourceInfo(format!("{e}")))?;
    let replacement: serde_json::Value = serde_json::from_str(modified_subtree_json)
        .map_err(|e| ApplyNodeEditError::DeserializeModifiedSubtree(format!("{e}")))?;
    let splice = serde_json::json!({
        "from": { "kind": "beforeNode", "si": si },
        "to":   { "kind": "afterNode",  "si": si },
        "replacement": replacement,
    });
    apply_node_splice(content, untransformed_ast_json, &splice.to_string())
}
```

Delete the old `apply_node_edit` body (the manual lookup/splice/reconcile) — it's now the shim above. Keep `decode_compact_source_info`, `read_completing_source_info` usage, `preserve_leaf_variant`.

- [ ] **Step 4: Run tests; confirm separators**

Run: `cargo nextest run -p pampa -E 'binary(integration) & test(node_edit_tests)'`
Expected: PASS — all `apply_node_splice_*`, all existing `apply_node_edit_*` (now routed through the shim). If a separator assertion fails, read the actual output, confirm it is *valid* qmd that round-trips, set the expected string to it, and note the observed separator behavior in the commit message.

- [ ] **Step 5: Run the full pampa suite** (monorepo regression guard)

Run: `cargo nextest run -p pampa`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/pampa/src/apply_node_edit.rs crates/pampa/tests/integration/node_edit_tests.rs
git commit -m "feat(pampa): apply_node_splice (boundary-addressed); apply_node_edit becomes a shim"
```

---

## Task 4: WASM — export `apply_node_splice`

**Files:**
- Modify: `crates/wasm-quarto-hub-client/src/lib.rs` (~line 2803, beside `apply_node_edit`)

**Interfaces:**
- Consumes: `pampa::apply_node_edit::apply_node_splice`.
- Produces: `#[wasm_bindgen] pub fn apply_node_splice(content, untransformed_ast_json, splice_json) -> String` returning the `AstResponse` JSON.

- [ ] **Step 1: Add the binding** (mirror the existing `apply_node_edit` wrapper exactly):

```rust
#[wasm_bindgen]
pub fn apply_node_splice(
    content: &str,
    untransformed_ast_json: &str,
    splice_json: &str,
) -> String {
    match pampa::apply_node_edit::apply_node_splice(content, untransformed_ast_json, splice_json) {
        Ok(qmd) => serde_json::to_string(&AstResponse {
            success: true, ast: None, qmd: Some(qmd),
            error: None, diagnostics: None, warnings: None,
        }).unwrap(),
        Err(e) => serde_json::to_string(&AstResponse {
            success: false, ast: None, qmd: None,
            error: Some(e.to_string()), diagnostics: None, warnings: None,
        }).unwrap(),
    }
}
```

Keep the existing `apply_node_edit` export (migration window).

- [ ] **Step 2: Build the workspace (native) to typecheck the crate**

Run: `cargo build -p wasm-quarto-hub-client`
Expected: PASS (compiles for host; WASM build happens in Task 9's verify).

- [ ] **Step 3: Commit**

```bash
git add crates/wasm-quarto-hub-client/src/lib.rs
git commit -m "feat(wasm): export apply_node_splice"
```

---

## Task 5: TS — Content/Boundary/Splice types + verb vocabulary

**Files:**
- Create: `ts-packages/preview-renderer/src/q2-preview/edit.ts`
- Test: `ts-packages/preview-renderer/src/q2-preview/edit.test.tsx`

**Interfaces:**
- Consumes: `BlockNode`, `SourceInfoJson` (string) from `../framework/types`.
- Produces: types `Content`, `ContainerRef`, `Boundary`, `Splice`; helpers `md`, `ast`, `EMPTY`; verbs `replaceNode`, `insertAfter`, `insertBefore`, `replaceRange`, `deleteNode`, `deleteRange`, `appendToDoc`, `prependToDoc`, `appendTo`, `prependTo`; container ctors `docRoot`, `nodeContainer`.

- [ ] **Step 1: Write the failing test**

```tsx
import { describe, it, expect } from 'vitest';
import {
  md, ast, replaceNode, insertAfter, insertBefore, replaceRange,
  deleteNode, appendToDoc, appendTo, nodeContainer,
} from './edit';

const SI = '{"t":0,"r":[1,2],"d":0}';
const SI2 = '{"t":0,"r":[5,6],"d":0}';

describe('verb lowering', () => {
  it('replaceNode → before(si)..after(si)', () => {
    expect(replaceNode(SI, md('x'))).toEqual({
      from: { kind: 'beforeNode', si: SI },
      to:   { kind: 'afterNode',  si: SI },
      content: { kind: 'markdown', text: 'x' },
    });
  });
  it('insertAfter → after(si)..after(si) (from==to)', () => {
    const s = insertAfter(SI, ast());
    expect(s.from).toEqual(s.to);
    expect(s.from).toEqual({ kind: 'afterNode', si: SI });
  });
  it('insertBefore → before(si)..before(si)', () => {
    const s = insertBefore(SI, md('x'));
    expect(s.from).toEqual({ kind: 'beforeNode', si: SI });
    expect(s.from).toEqual(s.to);
  });
  it('replaceRange → before(first)..after(last)', () => {
    const s = replaceRange(SI, SI2, md('x'));
    expect(s.from).toEqual({ kind: 'beforeNode', si: SI });
    expect(s.to).toEqual({ kind: 'afterNode', si: SI2 });
  });
  it('deleteNode → empty content span', () => {
    const s = deleteNode(SI);
    expect(s.content).toEqual({ kind: 'ast', blocks: [] });
    expect(s.from).toEqual({ kind: 'beforeNode', si: SI });
    expect(s.to).toEqual({ kind: 'afterNode', si: SI });
  });
  it('appendToDoc → endOf(docRoot) twice', () => {
    const s = appendToDoc(md('x'));
    expect(s.from).toEqual({ kind: 'endOf', container: { kind: 'docRoot' } });
    expect(s.from).toEqual(s.to);
  });
  it('appendTo(node) → endOf(node)', () => {
    const s = appendTo(nodeContainer(SI), ast());
    expect(s.from).toEqual({ kind: 'endOf', container: { kind: 'node', si: SI } });
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd ts-packages/preview-renderer && npx vitest run src/q2-preview/edit.test.tsx`
Expected: FAIL — `./edit` not found.

- [ ] **Step 3: Implement `edit.ts`**

```ts
import type { BlockNode } from '../framework/types';

export type SourceInfoJson = string; // JSON.stringify(resolveSource(node).sourceEntry)

export type Content =
  | { kind: 'markdown'; text: string }
  | { kind: 'ast'; blocks: BlockNode[] };

export const md = (text: string): Content => ({ kind: 'markdown', text });
export const ast = (...blocks: BlockNode[]): Content => ({ kind: 'ast', blocks });
export const EMPTY: Content = { kind: 'ast', blocks: [] };

export type ContainerRef =
  | { kind: 'docRoot' }
  | { kind: 'node'; si: SourceInfoJson };

export const docRoot: ContainerRef = { kind: 'docRoot' };
export const nodeContainer = (si: SourceInfoJson): ContainerRef => ({ kind: 'node', si });

export type Boundary =
  | { kind: 'beforeNode'; si: SourceInfoJson }
  | { kind: 'afterNode'; si: SourceInfoJson }
  | { kind: 'startOf'; container: ContainerRef }
  | { kind: 'endOf'; container: ContainerRef };

export interface Splice { from: Boundary; to: Boundary; content: Content }

const before = (si: SourceInfoJson): Boundary => ({ kind: 'beforeNode', si });
const after = (si: SourceInfoJson): Boundary => ({ kind: 'afterNode', si });
const startOf = (c: ContainerRef): Boundary => ({ kind: 'startOf', container: c });
const endOf = (c: ContainerRef): Boundary => ({ kind: 'endOf', container: c });

export const replaceNode = (si: SourceInfoJson, content: Content): Splice =>
  ({ from: before(si), to: after(si), content });
export const insertAfter = (si: SourceInfoJson, content: Content): Splice =>
  ({ from: after(si), to: after(si), content });
export const insertBefore = (si: SourceInfoJson, content: Content): Splice =>
  ({ from: before(si), to: before(si), content });
export const replaceRange = (firstSi: SourceInfoJson, lastSi: SourceInfoJson, content: Content): Splice =>
  ({ from: before(firstSi), to: after(lastSi), content });
export const deleteNode = (si: SourceInfoJson): Splice => replaceNode(si, EMPTY);
export const deleteRange = (firstSi: SourceInfoJson, lastSi: SourceInfoJson): Splice =>
  replaceRange(firstSi, lastSi, EMPTY);
export const appendTo = (c: ContainerRef, content: Content): Splice =>
  ({ from: endOf(c), to: endOf(c), content });
export const prependTo = (c: ContainerRef, content: Content): Splice =>
  ({ from: startOf(c), to: startOf(c), content });
export const appendToDoc = (content: Content): Splice => appendTo(docRoot, content);
export const prependToDoc = (content: Content): Splice => prependTo(docRoot, content);
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd ts-packages/preview-renderer && npx vitest run src/q2-preview/edit.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ts-packages/preview-renderer/src/q2-preview/edit.ts ts-packages/preview-renderer/src/q2-preview/edit.test.tsx
git commit -m "feat(preview-renderer): boundary-splice edit EDSL (Content + verbs)"
```

---

## Task 6: TS — `commit` on context + remove old commit fns

**Files:**
- Modify: `ts-packages/preview-renderer/src/q2-preview/PreviewContext.tsx`
- Modify: `ts-packages/preview-renderer/src/q2-preview/PreviewRoot.tsx`
- Modify: `ts-packages/preview-renderer/src/q2-preview/usePreviewEdit.ts`
- Modify: `ts-packages/preview-renderer/src/framework/dispatchers.tsx`

**Interfaces:**
- Consumes: `Splice`, verbs from `./edit`.
- Produces: `PreviewContext.commit?: (splice: Splice) => void`; `usePreviewEdit()` returns `{ resolveSource, commit }`.

- [ ] **Step 1: Update `PreviewContext.tsx`** — replace the two commit fields with one:

```ts
import type { Splice } from './edit';
// in the context type:
//   commitTextEdit?: ...        ← REMOVE
//   commitSubtreeEdit?: ...     ← REMOVE
    commit?: (splice: Splice) => void;
```

- [ ] **Step 2: Update `PreviewRoot.tsx`** — replace `commitTextEdit`/`commitSubtreeEdit` with `commit`:

```tsx
import type { Splice } from './edit';
const commit = (splice: Splice) => {
    // setAst is the parent prop callback; the Splice rides it (typed in Task 7).
    props.setAst(splice as unknown as PandocAST);
};
// provide { ...existing, commit } on the context; remove the two old fns.
```

- [ ] **Step 3: Update `usePreviewEdit.ts`**:

```ts
import type { Splice } from './edit';
export function usePreviewEdit(): {
    resolveSource: (node: BlockNode) => ResolvedSource | null;
    commit: (splice: Splice) => void;
} {
    const ctx = useContext(PreviewContext);
    return {
        resolveSource: ctx?.resolveSource ?? (() => null),
        commit: ctx?.commit ?? (() => undefined),
    };
}
```

- [ ] **Step 4: Update `dispatchers.tsx`** — `EditTextarea` commit + delete-by-emptying:

```tsx
import { replaceNode, deleteNode, md } from '../q2-preview/edit';
// where it previously called ctx.commitTextEdit!(dest, newText):
const text = newText; // raw qmd
if (text.trim() === '') ctx.commit?.(deleteNode(dest));
else ctx.commit?.(replaceNode(dest, md(text)));
```

(`dest` is the same `JSON.stringify(resolved.sourceEntry)` value used today.)

- [ ] **Step 5: Run the preview-renderer unit + integration suite**

Run: `cd ts-packages/preview-renderer && npx vitest run`
Expected: PASS. Fix any test that referenced `commitTextEdit`/`commitSubtreeEdit` to drive `commit` with a verb (these are *our* tests; update them — do not re-add the old API).

- [ ] **Step 6: Commit**

```bash
git add ts-packages/preview-renderer/src/q2-preview/PreviewContext.tsx \
        ts-packages/preview-renderer/src/q2-preview/PreviewRoot.tsx \
        ts-packages/preview-renderer/src/q2-preview/usePreviewEdit.ts \
        ts-packages/preview-renderer/src/framework/dispatchers.tsx
git commit -m "refactor(preview-renderer): usePreviewEdit -> { resolveSource, commit }; drop old commit fns"
```

---

## Task 7: Parent — route the Splice through `apply_node_splice`

**Files:**
- Modify: `hub-client/src/components/render/ReactPreview.tsx` (`handleSetAst`, ~653–718)
- Modify: `hub-client/src/types/wasm-quarto-hub-client.d.ts`

**Interfaces:**
- Consumes: WASM `apply_node_splice`; `Splice`/`Content` from preview-renderer; `parseQmdContentSync`.
- Produces: parent normalization of `Content` → Pandoc-JSON blocks, then `apply_node_splice` call.

- [ ] **Step 1: Declare the WASM export** in `wasm-quarto-hub-client.d.ts`:

```ts
export function apply_node_splice(
  content: string,
  untransformed_ast_json: string,
  splice_json: string,
): string;
```

- [ ] **Step 2: Update `handleSetAst`** to accept a `Splice`, normalize `content`, and call `apply_node_splice`:

```tsx
import { applyNodeSplice } from '...wasm wrapper...';
import type { Splice } from '@quarto/preview-renderer/.../edit';

const handleSetAst = useCallback((payload: any) => {
  if (pipelineKindForFormat(format) !== 'preview') { /* q2-debug/slides path unchanged */ return; }
  const splice = payload as Splice;
  if (!splice?.from || !splice?.to || !splice?.content) {
    console.warn('q2-preview setAst: expected Splice; got', payload); return;
  }
  if (!rendered.untransformedAstJson) {
    console.warn('q2-preview setAst: no untransformedAstJson retained; render first'); return;
  }
  beginCommitStatus();
  try {
    // Normalize Content → Pandoc-JSON blocks (markdown parsed here, at the edge).
    let replacement: any;
    if (splice.content.kind === 'markdown') {
      const parsed = parseQmdContentSync(splice.content.text);
      if (!parsed.success || !parsed.ast) {
        settleCommitStatus('error');
        setCommitError(`Edit could not be parsed: ${parsed.error ?? 'unknown'}`); return;
      }
      replacement = JSON.parse(parsed.ast); // full Pandoc doc
    } else {
      const stripped = splice.content.blocks.map(b =>
        JSON.parse(JSON.stringify(b, (k, v) => (k === 's' || k === 'a' ? undefined : v))));
      replacement = { 'pandoc-api-version': [1, 23, 0], meta: {}, blocks: stripped };
    }
    const wire = JSON.stringify({ from: splice.from, to: splice.to, replacement });
    const newQmd = applyNodeSplice(rendered.renderedContent, rendered.untransformedAstJson, wire);
    settleCommitStatus(newQmd === rendered.renderedContent ? 'spurious' : 'change');
    setCommitError(null);
    onContentRewrite(newQmd);
  } catch (err) {
    settleCommitStatus('error');
    setCommitError(`Edit could not be applied: ${err instanceof Error ? err.message : String(err)}`);
  }
}, [/* deps as today */]);
```

`applyNodeSplice` returns the `AstResponse` JSON like `applyNodeEdit`; unwrap `.qmd`/`.error` with the same wrapper the existing `applyNodeEdit` import uses (mirror it).

- [ ] **Step 3: Typecheck hub-client**

Run: `cd hub-client && npx tsc -b`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add hub-client/src/components/render/ReactPreview.tsx hub-client/src/types/wasm-quarto-hub-client.d.ts
git commit -m "feat(hub-client): route preview edits through apply_node_splice"
```

---

## Task 8: Update the three demo render components

**Files:**
- Modify: `~/docs/demo-playground/gordon/render-components2/drag.tsx`
- Modify: `~/docs/demo-playground/gordon/render-components2/kanban.tsx`
- Modify: `~/docs/demo-playground/gordon/render-components2/comment.tsx`

**Interfaces:**
- Consumes: `usePreviewEdit().commit`, `replaceNode`, `ast` from `window.__Q2_PREVIEW_RENDERER__`.

- [ ] **Step 1: Confirm the verbs are on the renderer surface.** Ensure `replaceNode`/`ast` (and `commit` via `usePreviewEdit`) are exported on `window.__Q2_PREVIEW_RENDERER__` (the surface assembled in preview-renderer's entry). If not, add them there and commit that with Task 6.

- [ ] **Step 2: drag.tsx** — replace the commit call:

```tsx
const { resolveSource, commit } = usePreviewEdit();
// ...
const resolved = resolveSource(args.node);
if (resolved) {
  const modified = structuredClone(resolved.sourceNode);
  modified.c[0][2] = [['x', x + ''], ['y', y + '']];
  commit(replaceNode(JSON.stringify(resolved.sourceEntry), ast(modified)));
}
```

- [ ] **Step 3: kanban.tsx** — same shape:

```tsx
const modified = structuredClone(resolved.sourceNode);
modified.c[1] = newBlocks;
commit(replaceNode(JSON.stringify(resolved.sourceEntry), ast(modified)));
```

- [ ] **Step 4: comment.tsx** — both `appendInlineToSource` and `removeFirstMatchingInSource`:

```tsx
commit(replaceNode(JSON.stringify(resolved.sourceEntry), ast(modified)));
```

- [ ] **Step 5: Commit** (these live outside the repo; commit in their own location if version-controlled, otherwise note the change in the hub-client commit). If they are not under git, skip the commit and record in the plan that they were updated in place.

---

## Task 9: End-to-end verification + full gate

**Files:** none new — verification only.

- [ ] **Step 1: Rebuild the WASM + preview SPA** (Rust changes do not reach the preview otherwise):

```bash
cd hub-client && npm run build:wasm
cd /Users/gordon/src/q2/.worktrees/block-editing && cargo xtask build-q2-preview-spa
```

- [ ] **Step 2: Re-run the render-component e2e specs** (real-browser proof the replace path survives the clean break):

Run: `cd hub-client && npx playwright test q2-preview-render-components-drag q2-preview-render-components-kanban q2-preview-render-components-comment`
Expected: PASS (drag move, kanban reorder, comment add/remove all round-trip through `commit(replaceNode(...))` → `apply_node_splice`).

- [ ] **Step 3: Harness e2e for the NEW ops** (insert + range have no production gesture yet). Add a focused integration test in preview-renderer that drives `commit(insertAfter(...))` and `commit(replaceRange(...))` through a mounted preview and asserts the resulting qmd. Mirror an existing `*.integration.test.tsx` in `ts-packages/preview-renderer/src/q2-preview/` that already exercises `commit`. Inspect the emitted qmd and assert the exact minimal text.

- [ ] **Step 4: Full verification gate**

Run: `cargo xtask verify`
Expected: PASS (workspace build + nextest + ts-packages build + hub-client build:all + hub tests).

- [ ] **Step 5: Record the end-to-end evidence** in this plan (per the repo's end-to-end-verification rule): the exact `commit(insertAfter(...))` invocation, the observed qmd diff, and a note that the output was inspected.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "test(block-editing): e2e for boundary-splice insert/range + render-component parity"
```

---

## Self-Review

- **Spec coverage:** verbs (Task 5) ↔ lowering table; `apply_node_splice` + resolver + `splice_range` (Tasks 1–3) ↔ Backend section; `ContainerRef = DocRoot | Node(si)` (Task 2 `resolve_container`) ↔ corrected spec; clean-break API (Tasks 6, 8) ↔ Migration; parent normalization (Task 7) ↔ three-levels §3; stale-AST degrade (Task 3 tests) ↔ degrade rules; `preserve_leaf_variant` narrowing (Task 1 terminal) ↔ spec. Item plane is out of scope by construction (no list/def container resolver).
- **Placeholder scan:** every code/test step carries real code; the one deferred judgment is the exact separator string in Task 3 Step 1, with an explicit instruction to confirm against the writer and pin the real value (not loosen the assertion).
- **Type consistency:** `apply_node_splice(content, untransformed_ast_json, splice_json)` is identical across Rust (Task 3), WASM (Task 4), and the `wire` built in the parent (Task 7). `Splice { from, to, content }` (TS, Task 5) → parent normalizes `content`→`replacement` → `SpliceWire { from, to, replacement }` (Rust, Task 3). `Boundary`/`ContainerRef` tags (`beforeNode`/`afterNode`/`startOf`/`endOf`; `docRoot`/`node`) match `#[serde(tag="kind", rename_all="camelCase")]`.

## Execution Handoff

Plan complete. Two execution options:

1. **Subagent-Driven (recommended)** — a fresh subagent per task, review between tasks.
2. **Inline Execution** — batch tasks in this session with checkpoints.
