# Shortcode Naked-String Widening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the `shortcode_naked_string` token accept everything that is not
a delimiter — non-ASCII, `\`-escapes, and the ASCII punctuation currently
excluded — so that a single unusual character in a shortcode argument stops
dropping the whole document.

**Architecture:** Replace the token's ASCII allowlist with a blocklist plus an
escape alternative (one regex in `grammar.js`), route the naked-string reader
through the existing, already-tested `text_helpers::extract_quoted_text`
unescaper instead of taking node text verbatim, and update the writer's mirror
predicate to match. `=` and `|` are deliberately handled specially; see
"Design decisions".

**Tech Stack:** tree-sitter (grammar + generated C parser), Rust (pampa reader
and qmd writer), `tree-sitter test` corpus, `cargo nextest`.

**Strands closed:** `bd-shortcode-escaped-gt-fatal-2u79bqp1` (escaped `\>`),
`bd-shortcode-naked-value-nonascii-47fzbmow` defect 1 (non-ASCII).

**Spec:** this document — it is self-contained; every measurement it relies on
is quoted inline.

## Global Constraints

- **`=` must remain excluded from the bare naked token.** Admitting it risks
  silently reinterpreting every existing `key=value` shortcode as a positional
  argument. The `?`-query second alternative at `grammar.js:688` stays
  byte-for-byte unchanged. Bare-`=` parity gap tracked as `bd-kx25ovmh`.
- **`|` must remain excluded from the *writer* predicate** even though the
  grammar accepts it. See "Design decisions → the `|` asymmetry".
- **Do not touch the local closure at `treesitter.rs:997-1010`.** That closure
  is *also* named `extract_quoted_text` and shadows the shared helper this plan
  adopts. It is the quoted-shortcode-arg unescaper; its `\\`-doubling
  round-trip bug is `bd-5te3iryt` and is out of scope. Whenever this plan says
  "`extract_quoted_text`" it means
  `treesitter_utils::text_helpers::extract_quoted_text` (`text_helpers.rs:72`),
  never the closure.
- **Every grammar change requires the full chain**: `tree-sitter generate`,
  `tree-sitter build`, `tree-sitter test`, then `cargo build`. The generated
  `src/parser.c`, `src/grammar.json` and `src/node-types.json` are committed
  artifacts.
- **The parse-error table must be regenerated in the same task as the grammar
  change** (Task 1 Step 8), not later. `_autogen-table.json` is keyed on
  tree-sitter parser state, and `tree-sitter generate` renumbers states. Doing
  it later would put a stale table underneath the Rust test gates in Tasks 2
  and 3, where a failure would be misattributed.
- Per-task gate: `cargo clippy -p <crate> --all-targets -- -D warnings` plus
  `cargo nextest run -p <crate>`. Workspace `cargo nextest run --workspace`
  once at the phase boundary (Task 5) and before any push.

## Design decisions (why the plan looks like this)

**Why a blocklist, not "allowlist plus non-ASCII word characters".** The
alternative fix direction suggested in `bd-shortcode-naked-value-nonascii-47fzbmow`
does not fix that strand's own repro. `Command-→` contains U+2192, whose Unicode
category is `Sm` — `char::is_alphanumeric()` returns **false** for it, as it does
for the entire Mac modifier-key family `⌘ ⌥ ⇧ ↩ ⌫` (all category `So`). A
word-character rule fixes `é` and `日` and leaves the real-world cases fatal.
Widening instead to "all non-ASCII" would leave an incoherent rule — `→` accepted
while `|` drops the document — and `* ^ | < > { } \` are all real keys on a US
keyboard, which is exactly what the `kbd` shortcode exists to document.

**Measured risk of the blocklist.** A prototype (since reverted) using the
**fallback** regex below — quotes excluded throughout — produced: `tree-sitter
generate` **zero conflicts**; `tree-sitter test` **599/601**, both failures being
`(parse error)` snapshots that assert `ERROR`-node shape for already-invalid
input; the first case of all corpus codes still resolving correctly against a
deliberately stale table. **These numbers belong to the fallback form.** The
primary form below differs by admitting interior quotes; its failure count is a
*prediction*, and Task 1 Step 5 says what to do if it differs.

**Why the reader change reuses `extract_quoted_text`.** That function already
applies CommonMark `\X`-collapses-when-punctuation semantics, already runs on
bare (unquoted) values, and is already unit-tested. The `key_value_value` path
(`treesitter.rs:1217`) already routes through it, which is why
`{{< kbd mac=Command-→ >}}` needs no reader change at all — only the positional
`shortcode_naked_string` arm takes text verbatim.

**Escape semantics match Q1 exactly.** Measured across 12 cases against
`quarto` 99.9.9:

| | Q1 naked | Q1 quoted |
|---|---|---|
| `\>` `\*` `\-` (punctuation) | collapses → `>` `*` `-` | preserved verbatim |
| `\n` (non-punctuation) | preserved | preserved |
| `\\` | — | collapses → `\` |
| `\"` / `\'` (enclosing quote) | — | **not a working escape** — mis-terminates |

The naked column is precisely the CommonMark rule that `unescape_punctuation`
(`text_helpers.rs:105`) implements, so Task 2 makes q2's naked path match Q1
character-for-character. The quoted column already matches what q2 does today,
**so the naked/quoted asymmetry is Q1's own design and is not a defect to fix
here.** Two footnotes: q2 handles `\"`/`\'` correctly where Q1 breaks
(`test_parse_escaped_double_quote`, `test_parse_escaped_single_quote`) and must
not regress to Q1's behaviour; and q2 does not collapse `\\` where Q1 does,
which is the one genuine divergence left in this area — `bd-5te3iryt`, out of
scope.

**Two producers, one node kind.** `shortcode_naked_string` is produced by *two*
paths, and Task 1 only widens one of them:
1. the internal regex at `grammar.js:687` (this plan's edit), and
2. the **external scanner** token `_language_specifier_token` (declared
   `grammar.js:1137`, implemented in `src/scanner.c:2159`,
   `parse_language_specifier`), aliased to `shortcode_naked_string` at
   `grammar.js:673`, over the narrower charset `[A-Za-z0-9_%.-]`.

The scanner runs **before** the internal lexer, so for a letter-initial argument
*it* decides, falling through to the widened regex only on a character outside
its own set (`return false`, `scanner.c:2220`). Practical consequences: an
executor debugging an unexpected parse will not find the whole answer in
`grammar.js`; and Task 2's reader change also runs on scanner-produced nodes,
where it is a harmless no-op (that charset contains no `\`).

**The `|` asymmetry — writer stays conservative.** The widened grammar accepts a
bare `|`, but the writer must keep quoting it. `write_shortcode_string_value`
(`qmd.rs:2215`) has no positional context, so a bare `|` would be emitted inside
a pipe-table cell, where it splits the cell and silently corrupts the row on
re-parse. This is not hypothetical: the motivating repro is a table row,
`| {{< kbd \> >}} | Switch to command mode |`. Today that round-trips *because*
the writer quotes. Verified on the current tree:

```
$ printf '| K | M |\n|---|---|\n| {{< kbd "|" >}} | pipe |\n' | pampa -t qmd
| K               | M    |
| --------------- | ---- |
| {{< kbd "|" >}} | pipe |
```

Quoting is always safe; not quoting is sometimes wrong. So the reader is
permissive (bare `|` works in hand-written source outside tables, as in Q1) and
the writer is conservative. `>` needs no special case — it stays excluded from
the naked charset, so the writer already quotes it.

**Why `size=2x` is not a valid test fixture.** `shortcode_number` is
`token(prec(3, …))` (`grammar.js:696`) and beats the naked token's `prec(1)` on
lexical precedence regardless of match length. So digit-initial values keep
erroring as `Q-2-34` after the widening — verified — and `{{< fa envelope
size=2x >}}` is a committed *error* fixture
(`resources/error-corpus/Q-2-34.json`), not a success case. Any test needing a
working `key=value` must use a non-digit value.

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js` | token definition | Modify lines 686-688 |
| `.../src/parser.c`, `src/grammar.json`, `src/node-types.json` | generated artifacts | Regenerated, committed |
| `.../test/corpus/shortcode.txt` | grammar corpus | Add tests; fix 2 recovery snapshots |
| `crates/pampa/resources/error-corpus/_autogen-table.json` | generated | Regenerated, committed |
| `crates/pampa/src/pandoc/treesitter.rs` | node dispatch | Modify line 984 + the `use` at 35-38 |
| `crates/pampa/src/pandoc/treesitter_utils/shortcode.rs` | arg construction | Add unescaping entry point |
| `crates/pampa/src/pandoc/treesitter_utils/text_helpers.rs` | shared unescaper | Doc-comment correction only |
| `crates/pampa/src/writers/qmd.rs` | writer mirror | Modify `is_shortcode_naked_char` (2245) + doc (2241) |
| `crates/pampa/tests/integration/test_shortcode.rs` | reader tests | Add tests |

---

## Phase 1 — Grammar

### Task 1: Widen the `shortcode_naked_string` token

**Files:**
- Modify: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:686-688`
- Test: `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/shortcode.txt`
- Regenerate: `crates/pampa/resources/error-corpus/_autogen-table.json`

**Interfaces:**
- Produces: a `shortcode_naked_string` node that may now contain non-ASCII,
  `\`-escape pairs, and the punctuation `* ^ |` etc. Node **kind name is
  unchanged**, so no Rust dispatch arm changes here.

- [x] **Step 0: Record the pre-change baseline**

This must run **before** any edit — it is the only true baseline, and Task 4
compares against it.

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-1
cargo build --bin pampa
cat > /tmp/error-code-check.sh <<'SH'
#!/bin/sh
# Re-check that each error-corpus code still resolves to itself.
cd /Users/gordon/src/q2/.worktrees/workspace-1
n=0
for f in crates/pampa/resources/error-corpus/Q-*.json; do
  code=$(jq -r '.code' "$f")
  content=$(jq -r '.cases[0].content // empty' "$f")
  [ -z "$content" ] && continue
  n=$((n+1))
  got=$(printf '%s\n' "$content" | ./target/debug/pampa -t native 2>&1 \
        | sed -e 's/\x1b\[[0-9;]*m//g' | grep -oE 'Q-[0-9]+-[0-9]+' | head -1)
  [ "$got" = "$code" ] || echo "MISMATCH $code -> ${got:-<none>}"
done
echo "checked $n codes"
SH
chmod +x /tmp/error-code-check.sh && /tmp/error-code-check.sh

cd crates/tree-sitter-qmd/tree-sitter-markdown && tree-sitter test 2>&1 | tail -1
```

Expected: no `MISMATCH` lines, `checked 33 codes` (there are 34 `Q-*.json`
files; one has no `cases[0].content` and is skipped). The corpus line reads
`Total parses: 601; successful parses: 601; failed parses: 0`. **Record the
"Total parses" number**, not the last test index — `tree-sitter test` also
prints a running index that reaches 604 and is not a count.

- [x] **Step 1: Write the failing corpus tests**

Append to `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/shortcode.txt`.
The file indents `(document` by 4 spaces; `tree-sitter test` normalizes either
way.

These expected trees were derived by parsing ASCII analogues on the current
grammar, not guessed — in particular `key_value_value` **wraps** a
`shortcode_naked_string` child in shortcode context (unlike attribute context,
where it is a leaf), and adjacent shortcodes have **no** `pandoc_space` sibling
because the shortcode node absorbs its own leading whitespace (which is why
`shortcode.rs:51-60` peels it back off).

```
================================================================================
naked shortcode arg: backslash-escaped closing angle (bd-shortcode-escaped-gt-fatal-2u79bqp1)
================================================================================
{{< kbd \> >}}
--------------------------------------------------------------------------------
    (document
      (section
        (pandoc_paragraph
          (shortcode
            (shortcode_delimiter)
            (shortcode_name)
            (shortcode_naked_string)
            (shortcode_delimiter)))))

================================================================================
naked shortcode arg: non-ASCII symbol (bd-shortcode-naked-value-nonascii-47fzbmow)
================================================================================
{{< kbd → >}}
--------------------------------------------------------------------------------
    (document
      (section
        (pandoc_paragraph
          (shortcode
            (shortcode_delimiter)
            (shortcode_name)
            (shortcode_naked_string)
            (shortcode_delimiter)))))

================================================================================
naked shortcode arg: accented letter
================================================================================
{{< kbd é >}}
--------------------------------------------------------------------------------
    (document
      (section
        (pandoc_paragraph
          (shortcode
            (shortcode_delimiter)
            (shortcode_name)
            (shortcode_naked_string)
            (shortcode_delimiter)))))

================================================================================
naked shortcode arg: CJK
================================================================================
{{< kbd 日 >}}
--------------------------------------------------------------------------------
    (document
      (section
        (pandoc_paragraph
          (shortcode
            (shortcode_delimiter)
            (shortcode_name)
            (shortcode_naked_string)
            (shortcode_delimiter)))))

================================================================================
naked shortcode arg: asterisk
================================================================================
{{< kbd * >}}
--------------------------------------------------------------------------------
    (document
      (section
        (pandoc_paragraph
          (shortcode
            (shortcode_delimiter)
            (shortcode_name)
            (shortcode_naked_string)
            (shortcode_delimiter)))))

================================================================================
naked shortcode arg: pipe
================================================================================
{{< kbd | >}}
--------------------------------------------------------------------------------
    (document
      (section
        (pandoc_paragraph
          (shortcode
            (shortcode_delimiter)
            (shortcode_name)
            (shortcode_naked_string)
            (shortcode_delimiter)))))

================================================================================
naked shortcode arg: interior apostrophe is content, not a quote
================================================================================
{{< kbd don't >}}
--------------------------------------------------------------------------------
    (document
      (section
        (pandoc_paragraph
          (shortcode
            (shortcode_delimiter)
            (shortcode_name)
            (shortcode_naked_string)
            (shortcode_delimiter)))))

================================================================================
naked shortcode arg: key=value still splits, non-ASCII value
================================================================================
{{< kbd mac=Command-→ >}}
--------------------------------------------------------------------------------
    (document
      (section
        (pandoc_paragraph
          (shortcode
            (shortcode_delimiter)
            (shortcode_name)
            (key_value_specifier
              (key_value_key)
              (key_value_value
                (shortcode_naked_string)))
            (shortcode_delimiter)))))
```

Note the URL-with-query case is deliberately **not** added — `grammar.js:688`
already handles it and `inline-shortcodes.txt:131` already covers it.

- [x] **Step 2: Run the corpus tests to verify the new ones fail**

```bash
cd crates/tree-sitter-qmd/tree-sitter-markdown
tree-sitter test
```

Expected: **all eight new tests fail**, each producing an `ERROR` node rather
than the expected shape. Total becomes 609 parses, 601 successful, 8 failed.
No previously-passing test may change.

- [x] **Step 3: Widen the token**

In `grammar.js`, replace **only the first alternative** on line 687 (and the
comment above it). Leave line 688 — the `?`-query alternative — exactly as is.

```js
        // Anything that is not whitespace, a shortcode/attr delimiter, or `=`
        // (the key/value separator), plus `\`-escape pairs. Quotes are excluded
        // only at the first character, where they would open a quoted string;
        // interior apostrophes are content (`don't`). Deliberately a blocklist:
        // an allowlist made every unenumerated character — all of Unicode
        // included — a fatal parse error that dropped the whole document
        // (bd-shortcode-escaped-gt-fatal-2u79bqp1, bd-shortcode-naked-value-nonascii-47fzbmow).
        // `=` stays excluded: admitting it would make `a=b` ambiguous with the
        // key/value production and could silently reinterpret existing
        // shortcodes. Bare-`=` parity gap is bd-kx25ovmh.
        // NOTE: this is not the only producer of `shortcode_naked_string` —
        // the external `_language_specifier_token` (scanner.c:2159) is aliased
        // to the same node kind and decides letter-initial arguments first.
        shortcode_naked_string: $ =>
            choice(token(prec(1, new RustRegex("(?:[^ \\t\\n\\r'\"<>{}=\\\\]|\\\\.)(?:[^ \\t\\n\\r<>{}=\\\\]|\\\\.)*"))),
                   token(prec(1, /(?:[A-Za-z0-9_.~:/?#\]@!$%&()+,;-]|\[)+[?](?:[A-Za-z0-9_.~:/?#\]@!%$&()+,;?=-]|\[)+/))),
```

- [x] **Step 4: Regenerate, rebuild, and run the corpus**

```bash
cd crates/tree-sitter-qmd/tree-sitter-markdown
tree-sitter generate && tree-sitter build && tree-sitter test
```

Expected: `generate` reports no conflicts; the eight new tests pass.

**Two pre-existing tests are predicted to fail** — `LaTeX and link clashes
(parse error)` (`inline-extension_latex.txt:50`) and `Q-2-35: 4-space indent
rejected (issue #184)` (`qmd.txt:914`). Both are `(parse error)` tests asserting
`ERROR`-node shape for input that is invalid either way; only the recovery
tokenization changes.

That two-failure prediction was **measured on the fallback regex below, not on
the primary form**, which additionally admits interior quotes. If you see a
*third* failure, first check whether its input contains an interior `'` or `"` —
that is the difference between the two forms. If so, either accept the new
snapshot (if the input is invalid either way and the `ERROR` node remains) or
switch to the fallback:

```js
new RustRegex("(?:\\\\.|[^ \\t\\n\\r'\"<>{}=\\\\])+")
```

If you switch, delete the `don't` corpus test from Step 1, note in the commit
message that interior apostrophes remain unsupported, and apply the
correspondingly-noted variant in Task 3 Steps 3-4.

Any failure whose input has **no** interior quote is unexplained — stop and
investigate rather than updating its snapshot.

- [x] **Step 5: Update the two error-recovery snapshots**

```bash
tree-sitter test -u
git diff test/corpus/
```

`-u` rewrites **every** currently-failing test, not a chosen subset — which is
safe only because Step 4 established that the sole remaining failures are the
two expected ones. Confirm the diff touches exactly those two blocks and that
both still assert an `ERROR` node — the inputs must remain errors; only the
tokens inside the `ERROR` change. Then re-run `tree-sitter test`: 609 parses,
609 successful.

- [x] **Step 6: Check the Rust side, including the error corpus**

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-1
cargo clippy -p tree-sitter-qmd --all-targets -- -D warnings
cargo nextest run -p pampa -E 'binary(integration) & test(test_error_corpus::)'
cargo nextest run -p pampa
```

`test_error_corpus_ariadne_output` (`test_error_corpus.rs:15`) asserts that all
**684** files under `resources/error-corpus/case-files/` still produce a located
diagnostic. **63 of them contain `{{<`.** If the widened token makes any of them
stop erroring, that test reddens here — and it is *not* something Task 2 fixes.

Analysis says the three shortcode error codes are safe: `Q-2-27`/`Q-2-28` are
unterminated shortcodes (`{{< hello` with no close — still an error), and
`Q-2-34` is governed by `shortcode_number`'s `prec(3)` (verified: `size=2x`
still reports `Q-2-34` under the widened parser). But a case file testing some
*other* code could contain a shortcode incidentally. If one newly passes,
diagnose it individually and record the finding; do not blanket-update.

Note that `test_error_corpus_text_snapshots` and `..._json_snapshots` provide no
protection here — they glob `resources/error-corpus/*.qmd`, which matches
nothing on disk, so they iterate zero files and pass unconditionally. That is a
pre-existing bug, filed as `bd-7bbdazug`, and out of scope.

- [x] **Step 7: Regenerate the parse-error state table**

```bash
cd crates/pampa
./scripts/build_error_table.ts
cd /Users/gordon/src/q2/.worktrees/workspace-1
cargo build --bin pampa
/tmp/error-code-check.sh
cargo nextest run -p pampa
```

Expected: `checked 33 codes`, no `MISMATCH` lines, pampa green. This runs here
rather than in a later task because `tree-sitter generate` renumbered the parser
states the table is keyed on; deferring it would leave a stale table underneath
Tasks 2 and 3's gates.

- [x] **Step 8: Commit**

```bash
git add crates/tree-sitter-qmd/tree-sitter-markdown/ crates/pampa/resources/error-corpus/
git commit -m "Widen shortcode_naked_string from an ASCII allowlist to a blocklist

The token's character set was exactly RFC 3986's URI repertoire minus ', * and
=, inherited when naked URLs were made to work. Every character outside it —
all non-ASCII included, since URIs are ASCII by definition — was a fatal parse
error that dropped the entire document.

Accepts anything that is not whitespace, a delimiter, or '=', plus backslash
escape pairs. '=' stays excluded so key=value is unaffected (bd-kx25ovmh).
Note the external scanner token (scanner.c:2159) is a second producer of this
node kind and is unchanged.

Two (parse error) corpus snapshots updated: both assert ERROR-node shape for
already-invalid input, and only the recovery tokenization changed.
_autogen-table.json regenerated — it is keyed on tree-sitter parser state.

Grammar half of bd-shortcode-escaped-gt-fatal-2u79bqp1 and
bd-shortcode-naked-value-nonascii-47fzbmow."
```

---

## Phase 2 — Reader

### Task 2: Unescape naked shortcode arguments

**Files:**
- Modify: `crates/pampa/src/pandoc/treesitter_utils/shortcode.rs` (add after :28)
- Modify: `crates/pampa/src/pandoc/treesitter.rs:984` and the `use` at :35-38
- Modify: `crates/pampa/src/pandoc/treesitter_utils/text_helpers.rs:29-35` (doc only)
- Test: `crates/pampa/tests/integration/test_shortcode.rs`

**Interfaces:**
- Consumes: `shortcode_naked_string` nodes that may now contain `\`-escapes
  (Task 1).
- Produces: `pub fn process_shortcode_naked_string(node: &tree_sitter::Node,
  input_bytes: &[u8], context: &ASTContext) -> PandocNativeIntermediate` in
  `treesitter_utils::shortcode`. `process_shortcode_string_arg` keeps its
  signature and stays the handler for `shortcode_name`.

- [x] **Step 1: Write the failing tests**

Append to `crates/pampa/tests/integration/test_shortcode.rs`. The helpers
`parse_qmd` (:13), `get_first_shortcode` (:33), `get_positional_strings` (:59)
and `get_keyword_arg` (:74) already exist, and `ShortcodeArg` is imported at :11.

```rust
// ============================================================================
// Naked-argument widening (bd-shortcode-escaped-gt-fatal-2u79bqp1,
// bd-shortcode-naked-value-nonascii-47fzbmow)
// ============================================================================

#[test]
fn test_naked_arg_escaped_gt_unescapes() {
    // The Positron docs case: `>` cannot be written bare because it would
    // close the shortcode, so it is escaped. Q1 hands the shortcode `>`.
    let pandoc = parse_qmd(r"{{< kbd \> >}}");
    let shortcode = get_first_shortcode(&pandoc);

    assert_eq!(shortcode.name, "kbd");
    assert_eq!(get_positional_strings(shortcode), vec![">"]);
}

#[test]
fn test_naked_arg_escaped_star_unescapes() {
    // Q1 collapses `\X` for any ASCII punctuation X, even where X needed no
    // escape. CommonMark semantics; `unescape_punctuation` implements them.
    let pandoc = parse_qmd(r"{{< kbd \* >}}");
    assert_eq!(get_positional_strings(get_first_shortcode(&pandoc)), vec!["*"]);
}

#[test]
fn test_naked_arg_backslash_before_non_punctuation_is_literal() {
    // `\n` is not an escape: n is not ASCII punctuation, so the backslash
    // survives verbatim. Passes both before and after the reader change;
    // it is a guard, not a red-to-green test.
    let pandoc = parse_qmd(r"{{< kbd \n >}}");
    assert_eq!(get_positional_strings(get_first_shortcode(&pandoc)), vec![r"\n"]);
}

#[test]
fn test_naked_arg_non_ascii_symbol() {
    let pandoc = parse_qmd("{{< kbd → >}}");
    assert_eq!(get_positional_strings(get_first_shortcode(&pandoc)), vec!["→"]);
}

#[test]
fn test_naked_arg_non_ascii_letters() {
    let pandoc = parse_qmd("{{< kbd é >}}");
    assert_eq!(get_positional_strings(get_first_shortcode(&pandoc)), vec!["é"]);

    let pandoc = parse_qmd("{{< kbd 日 >}}");
    assert_eq!(get_positional_strings(get_first_shortcode(&pandoc)), vec!["日"]);
}

#[test]
fn test_naked_arg_previously_excluded_punctuation() {
    for ch in ["*", "^", "|"] {
        let src = format!("{{{{< kbd {} >}}}}", ch);
        let pandoc = parse_qmd(&src);
        assert_eq!(
            get_positional_strings(get_first_shortcode(&pandoc)),
            vec![ch],
            "bare {ch} should be a positional naked arg"
        );
    }
}

#[test]
fn test_keyword_arg_non_ascii_value() {
    // bd-shortcode-naked-value-nonascii-47fzbmow's real-world input. The
    // key/value split must survive the widening.
    let pandoc = parse_qmd("{{< kbd mac=Command-→ win=Ctrl-→ >}}");
    let shortcode = get_first_shortcode(&pandoc);

    assert_eq!(shortcode.name, "kbd");
    assert!(
        shortcode.positional_args.is_empty(),
        "key=value must not collapse into a positional arg"
    );
    match get_keyword_arg(shortcode, "mac") {
        Some(ShortcodeArg::String(s)) => assert_eq!(s, "Command-→"),
        other => panic!("expected mac=Command-→, got {other:?}"),
    }
    match get_keyword_arg(shortcode, "win") {
        Some(ShortcodeArg::String(s)) => assert_eq!(s, "Ctrl-→"),
        other => panic!("expected win=Ctrl-→, got {other:?}"),
    }
}

#[test]
fn test_keyword_arg_still_splits_on_ascii() {
    // Regression guard for the highest-blast-radius risk of this change:
    // `=` must keep separating key from value.
    //
    // NB: the value must not start with a digit. `size=2x` is a Q-2-34 error
    // fixture (shortcode_number's prec(3) beats the naked token), and
    // parse_qmd would panic on it.
    let pandoc = parse_qmd("{{< fa envelope size=large >}}");
    let shortcode = get_first_shortcode(&pandoc);

    assert_eq!(get_positional_strings(shortcode), vec!["envelope"]);
    match get_keyword_arg(shortcode, "size") {
        Some(ShortcodeArg::String(s)) => assert_eq!(s, "large"),
        other => panic!("expected size=large, got {other:?}"),
    }
}

#[test]
fn test_naked_url_with_query_string_unchanged() {
    let pandoc = parse_qmd("{{< video https://x.com/v?t=1&u=2 >}}");
    let shortcode = get_first_shortcode(&pandoc);

    assert_eq!(
        get_positional_strings(shortcode),
        vec!["https://x.com/v?t=1&u=2"],
        "a query string must stay one naked token, not split on ="
    );
    assert!(shortcode.keyword_args.is_empty());
}

#[test]
fn test_shortcode_name_is_not_unescaped() {
    // shortcode_name keeps the verbatim path; guard that it still works.
    let pandoc = parse_qmd("{{< my_shortcode-2 arg >}}");
    assert_eq!(get_first_shortcode(&pandoc).name, "my_shortcode-2");
}
```

- [x] **Step 2: Run the tests to verify they fail**

```bash
cargo nextest run -p pampa -E 'binary(integration) & test(test_shortcode::)'
```

Expected failures, and no others:
- `test_naked_arg_escaped_gt_unescapes` — gets `\>`, wants `>`
- `test_naked_arg_escaped_star_unescapes` — gets `\*`, wants `*`

Everything else should already **pass** after Task 1: the non-ASCII and
punctuation cases needed only the grammar, and `test_keyword_arg_non_ascii_value`
passes because `key_value_value` already routes through `extract_quoted_text`.
If any of those fail, stop — Task 1 is incomplete.

- [x] **Step 3: Add the unescaping entry point**

In `crates/pampa/src/pandoc/treesitter_utils/shortcode.rs`, add the sibling
import next to the existing `use super::…` at line 15 (the file imports siblings
via `super::`, not the full crate path):

```rust
use super::text_helpers::extract_quoted_text;
```

Then add this after `process_shortcode_string_arg` (which stays exactly as it
is — it remains the handler for `shortcode_name`, whose grammar rule
`[a-zA-Z_][a-zA-Z0-9_-]*` cannot contain a backslash):

```rust
/// Process a `shortcode_naked_string` node, applying CommonMark
/// backslash-escape semantics.
///
/// Unlike [`process_shortcode_string_arg`] (used for `shortcode_name`, which
/// cannot contain a backslash), a naked argument may carry `\X` pairs since
/// the token was widened to a blocklist — `\>` is the only way to write a
/// literal `>`, which would otherwise close the shortcode.
///
/// Delegates to [`extract_quoted_text`], which already implements exactly this
/// rule and is the same decoder `key_value_value` uses, so a naked value and a
/// `key=value` value decode identically. Its quote-stripping cannot misfire
/// here: the grammar forbids a naked token from *starting* with a quote, and
/// stripping requires both ends to match.
///
/// Also reached by scanner-produced nodes — `_language_specifier_token`
/// (`scanner.c:2159`) is aliased to this node kind — where it is a no-op,
/// since that token's charset `[A-Za-z0-9_%.-]` contains no backslash.
pub fn process_shortcode_naked_string(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let raw = node.utf8_text(input_bytes).unwrap();
    let (decoded, _content_source) =
        extract_quoted_text(raw, context.current_file_id(), node.start_byte());
    let source_info = node_source_info_with_context(node, context);
    let range =
        crate::pandoc::location::source_info_to_qsm_range_or_fallback(&source_info, context);
    PandocNativeIntermediate::IntermediateShortcodeArg(ShortcodeArg::String(decoded), range)
}
```

`_content_source` is discarded because `ShortcodeArg::String` carries no
`SourceInfo`, so no consumer can offset into the decoded value — the same
reasoning already documented at `treesitter.rs:989-996`.

- [x] **Step 4: Dispatch naked strings to the new function**

In `crates/pampa/src/pandoc/treesitter.rs`, change **only** line 984, leaving
line 983 (`shortcode_name`) alone:

```rust
        "shortcode_name" => process_shortcode_string_arg(node, input_bytes, context),
        "shortcode_naked_string" => {
            process_shortcode_naked_string(node, input_bytes, context)
        }
```

Add `process_shortcode_naked_string` to the `use ...shortcode::{…}` block at
lines 35-38, alphabetized before `process_shortcode_string_arg`. (The repo's
post-tool-use hook runs `cargo fmt`, which may reflow the list — that is fine.)

- [x] **Step 5: Correct the now-false doc comment in `text_helpers.rs`**

`text_helpers.rs:29-35` currently justifies "a bare value is
unreachable-with-escapes today" by citing the scanner's charset. Task 1 makes
that false for shortcode arguments. Replace that clause so it reads:

```rust
/// otherwise the backslash is preserved literally. Escape processing runs
/// unconditionally, quoted or not. Bare values reach this function from two
/// places: attribute values, whose scanner token (`[A-Za-z0-9_%.-]`) still
/// excludes `\`; and shortcode naked strings, whose token was widened to admit
/// `\X` escape pairs (bd-shortcode-escaped-gt-fatal-2u79bqp1), which is why
/// `shortcode::process_shortcode_naked_string` routes through here. The
/// *quoted* shortcode form still has its own ad hoc `\"`/`\'`-only unescaper —
/// the local closure at `treesitter.rs:997` — which is why a quoted argument
/// and a naked one do not decode identically today (bd-5te3iryt).
```

- [x] **Step 6: Run the tests to verify they pass**

```bash
cargo nextest run -p pampa -E 'binary(integration) & test(test_shortcode::)'
cargo clippy -p pampa --all-targets -- -D warnings
```

Expected: all pass, clippy clean.

- [x] **Step 7: Commit**

```bash
git add crates/pampa/src/pandoc/treesitter.rs \
        crates/pampa/src/pandoc/treesitter_utils/shortcode.rs \
        crates/pampa/src/pandoc/treesitter_utils/text_helpers.rs \
        crates/pampa/tests/integration/test_shortcode.rs
git commit -m "Apply backslash-escape semantics to naked shortcode arguments

Routes shortcode_naked_string through text_helpers::extract_quoted_text, the
same decoder key_value_value already uses, so '\\>' reaches the shortcode as
'>' and a naked value decodes identically to a key=value value. Matches Q1,
which collapses '\\X' for ASCII punctuation X.

shortcode_name keeps the verbatim path; its grammar rule cannot produce a
backslash. Corrects text_helpers' doc comment, which justified its
'bare values never carry a backslash' claim from the scanner charset.

Reader half of bd-shortcode-escaped-gt-fatal-2u79bqp1."
```

---

## Phase 3 — Writer mirror

### Task 3: Update `is_shortcode_naked_char` to match the grammar

**Files:**
- Modify: `crates/pampa/src/writers/qmd.rs:2231` and `:2241-2269`
- Test: `crates/pampa/src/writers/qmd.rs`, `mod shortcode_writer_tests` (:2383)

**Interfaces:**
- Consumes: nothing from Tasks 1-2 at runtime; this is the writer's mirror of
  the grammar rule Task 1 changed.
- Produces: `fn is_shortcode_naked_char(c: char) -> bool` (same signature) and a
  new `fn is_shortcode_naked_first_char(c: char) -> bool`.

**Why this matters:** if the writer keeps the old predicate it will quote values
the grammar now accepts bare — safe, but it churns every round-tripped document
and makes the writer disagree with its own doc comment.

**Note the file has two test modules.** `mod tests` ends at :2380; the shortcode
assertions live in `mod shortcode_writer_tests` starting at :2383. Everything
below goes in the latter.

- [x] **Step 1: Write the failing tests**

Add to `mod shortcode_writer_tests`:

```rust
    #[test]
    fn non_ascii_does_not_need_quoting() {
        // Mirrors the widened grammar: non-ASCII is ordinary content.
        assert!(!shortcode_string_needs_quoting("Command-→"));
        assert!(!shortcode_string_needs_quoting("é"));
        assert!(!shortcode_string_needs_quoting("日本語"));
        assert!(!shortcode_string_needs_quoting("⌘"));
    }

    #[test]
    fn widened_punctuation_does_not_need_quoting() {
        assert!(!shortcode_string_needs_quoting("*"));
        assert!(!shortcode_string_needs_quoting("^"));
    }

    #[test]
    fn pipe_still_needs_quoting_despite_the_grammar_accepting_it() {
        // The grammar accepts a bare `|`, but the writer has no positional
        // context and would emit it inside a pipe-table cell, splitting the
        // row on re-parse. Quoting is always safe.
        assert!(shortcode_string_needs_quoting("|"));
        assert!(shortcode_string_needs_quoting("a|b"));
    }

    #[test]
    fn leading_quote_needs_quoting_interior_does_not() {
        // The grammar forbids a naked token from *starting* with a quote.
        assert!(shortcode_string_needs_quoting("'leading"));
        assert!(shortcode_string_needs_quoting("\"leading"));
        assert!(!shortcode_string_needs_quoting("don't"));
    }
```

Then edit the existing `delimiter_chars_must_be_quoted` (:2411-2418): delete the
two assertions `assert!(shortcode_string_needs_quoting("a\"b"));` and
`assert!(shortcode_string_needs_quoting("a'b"));` at :2415-2416, since interior
quotes become legal bare. Add `assert!(shortcode_string_needs_quoting("a<b"));`
while you are there. Do **not** add a second delimiter test — the existing one
plus `whitespace_must_be_quoted` and `empty_must_be_quoted` already cover the
rest.

**If Task 1 fell back** to the quotes-excluded-throughout regex: keep those two
assertions, and drop `leading_quote_needs_quoting_interior_does_not` entirely.

- [x] **Step 2: Run the tests to verify they fail**

```bash
cargo nextest run -p pampa -E 'test(writers::qmd::shortcode_writer_tests::)'
```

(That filter is exact. `binary(qmd)` does not exist — pampa's binaries are
`pampa`, `pampa::integration`, `pampa::wasm_lua`, `pampa::bin/*` — and
`test(writers::qmd)` also drags in `smart_typography_writer_tests`.)

Expected: **three** failures —
`non_ascii_does_not_need_quoting`,
`widened_punctuation_does_not_need_quoting`, and
`leading_quote_needs_quoting_interior_does_not` (which asserts
`!needs_quoting("don't")`, and the pre-change predicate excludes `'`).
`pipe_still_needs_quoting_despite_the_grammar_accepting_it` passes from the
start — it guards behaviour Step 3 must preserve.

Each test appears twice in the output (`pampa` and `pampa::bin/pampa`) because
pampa's unit tests compile into two binaries.

- [x] **Step 3: Rewrite the predicate and its doc comment**

Replace `crates/pampa/src/writers/qmd.rs:2241-2269`:

```rust
/// Characters allowed unquoted in a shortcode argument by the *writer*.
///
/// Mirrors the blocklist in grammar.js (`shortcode_naked_string`) with two
/// deliberate narrowings, both in the safe direction — the writer quotes more
/// than the parser strictly requires:
///
/// - `|` is excluded. The grammar accepts it, but this function has no
///   positional context, and a bare `|` emitted inside a pipe-table cell
///   splits the row on re-parse. The motivating repro is a table row.
/// - Whitespace is tested with `is_ascii_whitespace`, matching the grammar's
///   ASCII-only `[^ \t\n\r…]`. Non-ASCII whitespace is content, not
///   whitespace, per claude-notes/plans/2026-04-30-unicode-whitespace-handling.md.
///
/// Non-ASCII is otherwise ordinary content. This was formerly an ASCII
/// allowlist copied from RFC 3986's URI repertoire, which made every
/// non-ASCII character — and `* ^ |` — a fatal parse error
/// (bd-shortcode-escaped-gt-fatal-2u79bqp1,
/// bd-shortcode-naked-value-nonascii-47fzbmow).
///
/// Note the grammar is not the only acceptor: for letter-initial values the
/// external scanner token (`scanner.c:2159`, charset `[A-Za-z0-9_%.-]`) decides
/// first. It is strictly narrower, so quoting decisions made here remain valid.
fn is_shortcode_naked_char(c: char) -> bool {
    !c.is_ascii_whitespace() && !matches!(c, '<' | '>' | '{' | '}' | '=' | '\\' | '|')
}

/// Characters allowed as the *first* character of a naked token. The grammar
/// additionally forbids a leading quote there, since it would open a quoted
/// string.
fn is_shortcode_naked_first_char(c: char) -> bool {
    is_shortcode_naked_char(c) && !matches!(c, '\'' | '"')
}
```

- [x] **Step 4: Teach `shortcode_string_needs_quoting` about the first character**

Replace the body of `shortcode_string_needs_quoting` (`qmd.rs:2231`):

```rust
fn shortcode_string_needs_quoting(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return true; // empty string: the parser would fail to match
    };
    if shortcode_string_looks_like_number(s) {
        return true;
    }
    !is_shortcode_naked_first_char(first) || !chars.all(is_shortcode_naked_char)
}
```

**If Task 1 fell back** to the quotes-excluded-throughout regex: drop
`is_shortcode_naked_first_char`, add `'\'' | '"'` to the `matches!` in
`is_shortcode_naked_char`, and keep the original one-line body
`!s.chars().all(is_shortcode_naked_char)` after the empty and number checks.

- [x] **Step 5: Run the tests to verify they pass**

```bash
cargo nextest run -p pampa -E 'test(writers::qmd::shortcode_writer_tests::)'
cargo clippy -p pampa --all-targets -- -D warnings
```

Expected: all pass, clippy clean.

- [x] **Step 6: Check for round-trip churn**

```bash
cargo nextest run -p pampa
```

Expected: green. Snapshot tests that round-trip qmd may now emit previously
quoted values bare. **If any `.snap` changes, inspect every one** and confirm
the only difference is removed quotes around a value the grammar now accepts.
Per the repo's snapshot policy, report the count and summarize the change in the
commit message, and flag anything surprising. If a snapshot shows a value
crossing a table-cell or delimiter boundary, stop — that is the `|` hazard and
means Step 3's exclusion was dropped.

- [x] **Step 7: Commit**

```bash
git add crates/pampa/src/writers/qmd.rs
git commit -m "Mirror the widened naked-string rule in the qmd writer

is_shortcode_naked_char was an ASCII allowlist copied from RFC 3986; it now
mirrors the grammar's blocklist so the writer stops quoting values the parser
accepts bare. Adds is_shortcode_naked_first_char for the leading-quote
restriction.

'|' is deliberately still quoted even though the grammar accepts it: the writer
has no positional context and a bare '|' inside a pipe-table cell splits the row
on re-parse.

Writer half of bd-shortcode-escaped-gt-fatal-2u79bqp1 and
bd-shortcode-naked-value-nonascii-47fzbmow."
```

---

## Phase 4 — Verification

### Task 4: End-to-end verification and phase gate

**Files:** none modified.

- [x] **Step 1: Build the real binary**

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-1
cargo build --bin q2
```

- [x] **Step 2: Run both strands' committed repros through the CLI**

Per the repo's end-to-end rule, tests passing is not sufficient — drive the
binary a user would run and inspect the output. Remove the stale `_site/` first
so you cannot read a previous run's output.

```bash
Q2=/Users/gordon/src/q2/.worktrees/workspace-1/target/debug/q2

cd /Users/gordon/src/q2-positron-docs/llms-info/repros/shortcode-escaped-gt-fatal/
rm -rf _site && $Q2 render; echo "rc=$?"
grep -o '<kbd[^<]*</kbd>' _site/index.html

cd /Users/gordon/src/q2-positron-docs/llms-info/repros/kbd-unquoted-param/
rm -rf _site && $Q2 render; echo "rc=$?"
grep -o '<kbd[^>]*>' _site/index.html
```

Expected: both exit 0 with no errors — the second previously reported "Rendered
1 of 3 files … 2 errors" and produced only `control.html`. The first must show
`<kbd title=">"`, not `title="\>"`. The second must show `data-mac="Command-→"`
and `data-windows="Ctrl-→"`.

Paste the exact invocations and observed output into the Verification section
below.

- [x] **Step 3: Compare against Q1**

Q1 (the system `quarto`) is the intended comparison here — this is a parity
check against TypeScript Quarto, **not** the docs site, so the repo rule
"always render docs/ with q2, never `quarto`" does not apply. Both repro dirs
already contain a committed `_site-q1/`, so re-rendering is optional.

```bash
cd /Users/gordon/src/q2-positron-docs/llms-info/repros/shortcode-escaped-gt-fatal/
diff <(grep -o '<kbd[^<]*</kbd>' _site/index.html) \
     <(grep -o '<kbd[^<]*</kbd>' _site-q1/index.html)
```

Expected: the `title` attribute matches Q1 (`title=">"`). Q1 emits `&gt;` as the
element text where q2 emits a raw `>` — an HTML-escaping difference in
`resources/extensions/quarto/kbd/kbd.lua`, which builds the element as an
unescaped `RawInline`. That is **not** part of this plan. If it is the only
remaining difference, record it and file a strand.

- [x] **Step 4: Workspace test suite (phase gate)**

```bash
cd /Users/gordon/src/q2/.worktrees/workspace-1
cargo nextest run --workspace
```

Expected: green. Report the pass/skip counts and the delta against the live
baseline on `main` — do not copy a figure from an older document.

Expected delta is **+18**: 10 new integration tests (counted once, in binary
`integration`) plus 4 new writer unit tests counted **twice**, because pampa's
unit tests compile into both `pampa` and `pampa::bin/pampa`. The 8 new grammar
corpus tests do not appear in the nextest count at all.

- [x] **Step 5: Lint and full verification**

```bash
cargo xtask lint
cargo xtask verify
```

Full `verify`, not `--skip-hub-build`: `pampa` is in `wasm-quarto-hub-client`'s
dependency closure, so the WASM leg can break even when the workspace build is
clean.

- [x] **Step 6: Reconcile the plan and update the strands**

Re-read this file, verify each `- [x]` against what actually landed, correct any
that are wrong, fill in the Verification section, then commit.

> **Note (Task 4 execution):** the plan reconciliation and Verification
> section below were completed. The two `braid close` commands were
> **intentionally not run** by the verification agent — they were reserved
> for the coordinator to run after reviewing this task's results. See the
> Verification section for the full evidence trail.

```bash
braid close bd-shortcode-escaped-gt-fatal-2u79bqp1 \
  --reason "Naked token widened to a blocklist with escape support; \\> now reaches the shortcode as >"
braid close bd-shortcode-naked-value-nonascii-47fzbmow \
  --reason "Defect 1 (non-ASCII fatal in naked args) fixed by the same blocklist widening. Defect 2 (state-keyed error-code misattribution) survives this change and is tracked independently as bd-kaa2jzf9."
```

- [x] **Step 7: Commit the plan**

```bash
git add claude-notes/plans/2026-08-24-shortcode-naked-string-widening.md
git commit -m "Plan: reconcile the checklist with what landed (bd-shortcode-escaped-gt-fatal-2u79bqp1)"
```

---

## Out of scope

| Strand | Why not here |
|---|---|
| `bd-5te3iryt` | Quoted-arg `\\` doubling in the local closure at `treesitter.rs:997`. Pre-existing and reachable on `main` today. **Correction (post-implementation, from the final whole-branch review):** an earlier draft of this row claimed "this plan's naked path never puts a bare backslash in the AST." That is **false**, and this plan's own test `test_naked_arg_backslash_before_non_punctuation_is_literal` (`test_shortcode.rs:527`) disproves it — `\X` where `X` is *not* ASCII punctuation preserves the backslash, so `{{< kbd \n >}}` yields the AST value `\n`. Measured round-trip: `{{< kbd \n >}}` → `{{< kbd "\\n" >}}` → `{{< kbd "\\\\n" >}}`. So bare arguments are now a **second entry point** into that strand's doubling, alongside the quoted route. It stays out of scope anyway, on different grounds: every affected input is *strictly better off* than before this plan, because the bare form was previously a fatal whole-document parse error and is now merely mis-round-tripped. Q1 has the same naked/quoted asymmetry, so reproducing it is correct, not a regression — the one real divergence is the `\\` collapse, which is that strand, whose title and description need widening to cover the bare path. |
| `bd-kaa2jzf9` | The `{state, sym}` error-code keying. Survives this change entirely; affects far more than shortcodes. Task 1 Step 7 only regenerates the table. |
| `bd-kx25ovmh` | Bare `=`. Requires reasoning about the key/value boundary, encoded in four places with three different formulations. After this plan it degrades quietly instead of fatally. |
| `bd-7bbdazug` | `test_error_corpus_text_snapshots` / `..._json_snapshots` glob a path matching nothing and iterate zero files. Pre-existing; noted in Task 1 Step 6 because it explains why only one corpus test actually guards this change. |
| `kbd` HTML escaping | `kbd.lua` builds `<kbd title="…">` as an unescaped `RawInline`, so q2 emits a raw `>` where Q1 emits `&gt;`. After Task 3 an interior `"` also becomes reachable from bare source, which can emit malformed HTML. File separately if confirmed at Task 4 Step 3. |

## Verification

Filled in at Task 4, on branch `braid/bd-shortcode-escaped-gt-fatal-2u79bqp1-shortcode-escaped-gt`,
HEAD `2477d2816` (Task 3's commit). Full detail, including a table of extra
error-code-attribution probes (Step 2b) and a discussion of two verification
anomalies, is in `.superpowers/sdd/2026-08-24-shortcode-naked-string-widening/task-4-report.md`.

**Step 1 — build:** `cargo build --bin q2` — clean, no errors.

**Step 2 — repros (output read directly, not inferred from exit code):**

Repro 1 (`shortcode-escaped-gt-fatal`):
```
Q2=/Users/gordon/src/q2/.worktrees/workspace-1/target/debug/q2
cd /Users/gordon/src/q2-positron-docs/llms-info/repros/shortcode-escaped-gt-fatal/
rm -rf _site && $Q2 render; echo "rc=$?"
grep -o '<kbd[^<]*</kbd>' _site/index.html
```
→ `rc=0`; `<kbd title=">" aria-hidden="true" >></kbd>` — `title=">"` confirmed
(not `title="\>"`).

Repro 2 (`kbd-unquoted-param`):
```
cd /Users/gordon/src/q2-positron-docs/llms-info/repros/kbd-unquoted-param/
rm -rf _site && $Q2 render; echo "rc=$?"
grep -o '<kbd[^>]*>' _site/index.html
```
→ `rc=0`, "Rendered 3 of 3 files" (was "1 of 3 … 2 errors"); output contains
`data-mac="Command-→"` and `data-windows="Ctrl-→"` — both confirmed present
by reading the grepped line.

**Step 2b — `bd-kaa2jzf9` survival check:** probed 5 post-widening inputs
against `./target/debug/pampa -t native`. Finding: the widening does **not**
close `bd-kaa2jzf9`. `{{< hello` (a truly unterminated shortcode, no `>}}`
anywhere) still reports `Q-2-27` "Line Break Before Shortcode Close" —
demonstrably wrong wording for an unterminated shortcode. Three other probes
(`{{< kbd = >}}`, `{{< kbd > >}}`, `{{< kbd "x >}}`) now fail with **no**
catalog code at all (generic "unexpected character or token" parse error) —
a different but related symptom. The digit-initial control case
(`{{< fa envelope size=2x >}}`) correctly still reports `Q-2-34`. Full table
in the Task 4 report.

**Step 3 — Q1 comparison:**
```
diff <(grep -o '<kbd[^<]*</kbd>' _site/index.html) \
     <(grep -o '<kbd[^<]*</kbd>' _site-q1/index.html)
```
→ `title=">"` matches Q1 exactly on both sides. Only remaining difference:
q2 emits raw `>` as element text, Q1 emits `&gt;` — the pre-identified,
out-of-scope `kbd.lua` unescaped-`RawInline` issue. (A whitespace-only
difference in the opening tag was also noticed and is unrelated/pre-existing;
see the Task 4 report.)

**Step 4 — workspace nextest (phase gate):**
```
cargo nextest run --workspace
```
→ `Summary [216.666s] 13148 tests run: 13148 passed (1 leaky), 199 skipped`.
Zero failures. Baseline `main` (`596ceb572`) measured via an isolated
detached worktree (`cargo nextest list --workspace --message-format json`):
13130 would-run + 199 ignored = 13329 total. **Delta: 13148 − 13130 = +18**
(skipped count unchanged), matching the plan's prediction exactly — 10 new
integration tests in `crates/pampa/tests/integration/test_shortcode.rs`
(Task 2, verified by grepping the commit diff for `#[test]` additions)
counted once, plus 4 new writer unit tests in `crates/pampa/src/writers/qmd.rs`
(Task 3, same method) counted twice (compiled into both `pampa` and
`pampa::bin/pampa`). This was cross-checked against an independent
diff-scoped derivation (Task 1 touches zero `.rs` files, so pampa's count
after Task 1 equals `main`'s: 4572/2 skipped; pampa after Task 3 is 4590;
4590 − 4572 = +18, implying the same `main` total of 13329) — both methods
agree exactly. See the Task 4 report for a fuller discussion, including a
noted disagreement about whether the isolated-worktree measurement is fully
trustworthy; the two independent derivations landing on identical numbers is
the strongest evidence either way.

**Step 5 — lint and full verify:**
`cargo xtask lint` → `All checks passed! (1042 files checked)`.
`cargo xtask verify` (full, not `--skip-hub-build`) → **passed on the second
attempt**: `✓ All verification steps passed!`, all 14/14 steps, WASM/hub leg
included (hub-client vitest file counts 89, 15, 22, 3, 40 (+2 skipped), 51, 8,
21, 22, all passed); tree-sitter corpus inside verify reported
`Total parses: 609; successful parses: 609; failed parses: 0`. The **first**
attempt reported 10 corpus failures at Step 4/14 (all in the exact tests this
plan added or touches) and exited 1; this did not reproduce in three
immediate direct `tree-sitter test` reruns (609/609 each) nor in a zero-diff
`tree-sitter generate` against the committed parser — treated as a one-off,
unexplained flake rather than a real regression. Full detail, including why
the first attempt's backtrace is not taken as evidence of a crash, is in the
Task 4 report.

**Output was inspected directly** for every claim above — repro HTML was read
via `grep`, not inferred from exit codes; nextest/verify/lint summaries were
read from their actual log output; the Step 2b table was built from actual
`pampa` stdout for each probed input.
