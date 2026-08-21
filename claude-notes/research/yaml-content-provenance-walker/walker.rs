use quarto_yaml::parse;

#[derive(Debug)]
struct Piece {
    src: (usize, usize),
    out: usize,
    verbatim: bool,
}
#[derive(Debug)]
enum Desync {
    Char { vi: usize, si: usize },
    ValueLeft(usize),
}

fn push(p: &mut Vec<Piece>, src: (usize, usize), out: usize, verbatim: bool) {
    // Zero-content pieces are STORED, not dropped: the piece list must tile its
    // source contiguously or `preimage_in` yields no hull (plan § The shared
    // builder). Dropping a deleted escaped break leaves exactly such a gap.
    if verbatim {
        if let Some(l) = p.last_mut() {
            if l.verbatim && l.src.1 == src.0 {
                l.src.1 = src.1;
                l.out += out;
                return;
            }
        }
    }
    p.push(Piece { src, out, verbatim });
}

fn walk(
    raw: &str,
    val: &str,
    indent: usize,
    esc: Option<char>,
    wide_entry: bool,
) -> Result<Vec<Piece>, Desync> {
    let (rb, vb) = (raw.as_bytes(), val.as_bytes());
    let mut pieces: Vec<Piece> = vec![];
    let (mut si, mut vi) = (0usize, 0usize);

    while vi < vb.len() {
        // Rule 1's entry is style-conditional (fix round 2,
        // 2026-08-21/22): flow styles (`wide_entry`) strip trailing
        // whitespace before a break, so it belongs to the fold and entry
        // must fire from the whitespace run's own leading edge, not only
        // at the `\n`/`\r`. Block styles keep trailing whitespace as
        // content (measured: `block_pipe_trailing_spaces_last_line` and
        // the `>` probe both derive correctly under the narrow, unwidened
        // entry — see the README), so they keep the original entry test.
        let enter = si < rb.len()
            && matches!(vb[vi], b' ' | b'\n' | b'\t')
            && if wide_entry {
                (rb[si] as char).is_whitespace()
            } else {
                rb[si] == b'\n' || rb[si] == b'\r'
            };
        if enter {
            let mut se = si;
            let mut nl = 0;
            while se < rb.len() && (rb[se] as char).is_whitespace() {
                if rb[se] == b'\n' {
                    nl += 1;
                }
                se += 1;
            }
            // A wide entry can land on a whitespace run with no newline at
            // all (plain literal internal spaces); fall through to
            // escape/verbatim/synthesis when there's nothing to fold. A
            // narrow (block) entry always has nl >= 1, since it only fires
            // on a real `\n`/`\r`.
            if nl > 0 {
                if indent > 0 {
                    let last_nl = raw[si..se].rfind('\n').map(|i| si + i + 1).unwrap_or(si);
                    se = se.min(last_nl + indent);
                }
                let mut ve = vi;
                while ve < vb.len() && matches!(vb[ve], b' ' | b'\n') {
                    ve += 1;
                }
                ve = ve.min(vi + nl.max(1));
                let identical = raw.as_bytes()[si..se] == val.as_bytes()[vi..ve];
                push(&mut pieces, (si, se), ve - vi, identical);
                si = se;
                vi = ve;
                continue;
            }
        }
        if let Some(e) = esc {
            if si < rb.len() && rb[si] == e as u8 {
                let (slen, olen) = escape_len(&raw[si..], e);
                if slen > 0 {
                    push(&mut pieces, (si, si + slen), olen, false);
                    si += slen;
                    vi += olen;
                    continue;
                }
            }
        }
        if si < rb.len() && rb[si] == vb[vi] {
            push(&mut pieces, (si, si + 1), 1, true);
            si += 1;
            vi += 1;
            continue;
        }
        if si >= rb.len() {
            if vb[vi..].iter().all(|c| *c == b'\n') {
                push(&mut pieces, (si, si), vb.len() - vi, false);
                return Ok(pieces);
            }
            return Err(Desync::ValueLeft(vi));
        }
        return Err(Desync::Char { vi, si });
    }
    Ok(pieces)
}

fn escape_len(tail: &str, e: char) -> (usize, usize) {
    let b = tail.as_bytes();
    if e == '\'' {
        return if b.len() >= 2 && b[1] == b'\'' {
            (2, 1)
        } else {
            (0, 0)
        };
    }
    match b.get(1) {
        Some(b'n' | b't' | b'r' | b'0' | b'a' | b'b' | b'"' | b'\\' | b'/') => (2, 1),
        Some(b'x') => (4, 1),
        Some(b'u') => {
            let cp = u32::from_str_radix(&tail[2..6.min(tail.len())], 16).unwrap_or(0);
            (6, char::from_u32(cp).map_or(1, |c| c.len_utf8()))
        }
        Some(b'\n' | b'\r') => {
            let mut i = 1;
            while i < b.len() && (b[i] as char).is_whitespace() {
                i += 1;
            }
            (i, 0)
        }
        _ => (0, 0),
    }
}

fn emit(label: &str, src: &str, block: bool, esc: Option<char>) {
    let y = parse(src).unwrap();
    let v = y.get_hash_value("k").unwrap();
    emit_node(label, v, src, block, esc, None);
}

/// Like `emit`, but takes an already-resolved node (a hash key, a flow-item,
/// ...) instead of looking one up under `"k"` — and an optional `val`
/// override for scalars whose decoded value isn't recoverable via
/// `Yaml::as_str()` (`~`, `true`: the event's value string is what
/// content provenance means, not the resolved `Yaml`; see
/// `content_source_info`'s doc comment).
fn emit_node(
    label: &str,
    v: &quarto_yaml::YamlWithSourceInfo,
    src: &str,
    block: bool,
    esc: Option<char>,
    val_override: Option<&str>,
) {
    let (s0, e0) = (v.source_info.start_offset(), v.source_info.end_offset());
    let quoted = esc.is_some() && matches!(src.as_bytes()[s0], b'\'' | b'"');
    // If a block scalar's marker points at the `|`/`>` header rather than at
    // content (the empty-body case), start the walk after the header line.
    let val_probe = v.yaml.as_str().unwrap_or("");
    let hdr =
        block && matches!(src.as_bytes()[s0], b'|' | b'>') && val_probe.bytes().all(|c| c == b'\n');
    let start = if hdr {
        src[s0..].find('\n').map(|i| s0 + i).unwrap_or(e0)
    } else {
        s0
    };
    let raw = if quoted {
        &src[s0 + 1..e0 - 1]
    } else if block {
        &src[start..]
    } else {
        &src[s0..e0]
    };
    let base = if quoted { s0 + 1 } else { start };
    let val = val_override.unwrap_or_else(|| v.yaml.as_str().unwrap_or(""));
    let indent = if block {
        s0 - src[..s0].rfind('\n').map(|i| i + 1).unwrap_or(0)
    } else {
        0
    };
    let style = if block {
        "block"
    } else if quoted {
        "quoted"
    } else {
        "plain"
    };

    let (verdict, cells) = match walk(raw, val, indent, esc, !block) {
        Ok(p) => {
            let total: usize = p.iter().map(|x| x.out).sum();
            let mut ok = total == val.len();
            let mut off = 0;
            for x in &p {
                if x.verbatim && raw.get(x.src.0..x.src.1) != val.get(off..off + x.out) {
                    ok = false;
                }
                off += x.out;
            }
            let mut cells = vec![];
            let mut off = 0;
            for x in &p {
                cells.push(format!(
                    "`{}..{}`<-`{}..{}`{}",
                    off,
                    off + x.out,
                    base + x.src.0,
                    base + x.src.1,
                    if x.verbatim { "" } else { "*" }
                ));
                off += x.out;
            }
            if p.is_empty() {
                cells.push("**none**".into());
            }
            (
                if ok { "ok" } else { "**TILE FAIL**" }.to_string(),
                cells.join(" "),
            )
        }
        Err(d) => ("**DESYNC**".to_string(), format!("`{d:?}`")),
    };
    println!(
        "| `{}` | {} | {} | `\"{}\"` | {}..{} | `\"{}\"` | {} | {} |",
        label,
        style,
        indent,
        src.escape_debug(),
        s0,
        e0,
        val.escape_debug(),
        cells,
        verdict
    );
}

fn hdr() {
    println!(
        "| shape | style | indent | source | span | decoded value | expected pieces (`content`<-`source`, `*` = replacement) | |"
    );
    println!("|---|---|---|---|---|---|---|---|");
}

fn root(label: &str, src: &str) {
    let y = parse(src).unwrap();
    let val = y.yaml.as_str().unwrap_or("");
    let (s0, e0) = (y.source_info.start_offset(), y.source_info.end_offset());
    let raw = &src[s0..e0];
    match walk(raw, val, 0, None, true) {
        Ok(p) => {
            let total: usize = p.iter().map(|x| x.out).sum();
            let mut ok = total == val.len();
            let mut off = 0;
            for x in &p {
                if x.verbatim && raw.get(x.src.0..x.src.1) != val.get(off..off + x.out) {
                    ok = false;
                }
                off += x.out;
            }
            let mut cells = vec![];
            let mut off = 0;
            for x in &p {
                cells.push(format!(
                    "`{}..{}`<-`{}..{}`{}",
                    off,
                    off + x.out,
                    s0 + x.src.0,
                    s0 + x.src.1,
                    if x.verbatim { "" } else { "*" }
                ));
                off += x.out;
            }
            println!(
                "| `{}` | plain (root) | 0 | `\"{}\"` | {}..{} | `\"{}\"` | {} | {} |",
                label,
                src.escape_debug(),
                s0,
                e0,
                val.escape_debug(),
                cells.join(" "),
                if ok { "ok" } else { "**BYTE MISMATCH**" }
            );
        }
        Err(d) => println!(
            "| `{}` | plain (root) | 0 | `\"{}\"` | | | `{:?}` | **DESYNC** |",
            label,
            src.escape_debug(),
            d
        ),
    }
}

fn main() {
    let b = true;
    let f = false;
    hdr();
    emit("block | single-line", "k: |\n  aaa\n", b, None);
    emit("block | 3 lines", "k: |\n  aaa\n  bbb\n  ccc\n", b, None);
    emit("block | no final newline", "k: |\n  aaa", b, None);
    emit("block |- strip", "k: |-\n  aaa\n  bbb\n", b, None);
    emit("block |+ keep", "k: |+\n  aaa\n\n\n", b, None);
    emit(
        "block | blank line inside",
        "k: |\n  aaa\n\n  bbb\n",
        b,
        None,
    );
    emit(
        "block | more-indented line",
        "k: |\n  aaa\n    bbb\n  ccc\n",
        b,
        None,
    );
    emit("block |2 indicator", "k: |2\n    aaa\n    bbb\n", b, None);
    emit(
        "block | trailing spaces on last line",
        "k: |\n  aaa\n  bbb   \n",
        b,
        None,
    );
    emit("block | CRLF", "k: |\r\n  aaa\r\n  bbb\r\n", b, None);
    emit("block | tab in content", "k: |\n  a\tb\n", b, None);
    emit(
        "block | content starts with `|`",
        "k: |\n  |pipe\n",
        b,
        None,
    );
    emit("block | content is exactly `|`", "k: |\n  |\n", b, None);
    emit(
        "block > fold + blank line",
        "k: >\n  aaa\n  bbb\n\n  ccc\n",
        b,
        None,
    );
    emit(
        "block > more-indented (not folded)",
        "k: >\n  aaa\n    bbb\n",
        b,
        None,
    );
    emit("plain single-line", "k: hello\n", f, None);
    emit("plain multi-line", "k: aaa\n  bbb\n  ccc\n", f, None);
    // Fix round 2 (2026-08-22): a plain scalar's trailing space is stripped
    // before the line-break fold, so it belongs to rule 1's break region —
    // not to a separate verbatim byte via rule 3, which desynced under the
    // narrow (pre-fix) entry condition. This is the shape that caught it.
    emit(
        "plain multi-line, trailing space before fold",
        "k: a \n  b\n",
        f,
        None,
    );
    emit("plain multi-line CRLF", "k: aaa\r\n  bbb\r\n", f, None);
    emit("single-quoted", "k: 'hello'\n", f, Some('\''));
    emit("single-quoted with ''", "k: 'it''s'\n", f, Some('\''));
    emit("single-quoted trailing ''", "k: 'its'''\n", f, Some('\''));
    emit("single-quoted all-escape", "k: ''''\n", f, Some('\''));
    emit("double-quoted \\t", "k: \"a\\tb\"\n", f, Some('\\'));
    emit("double-quoted \\u00e9", "k: \"a\\u00e9b\"\n", f, Some('\\'));
    emit(
        "double-quoted many escapes",
        "k: \"a\\\\b\\\"c\\td\"\n",
        f,
        Some('\\'),
    );
    emit(
        "double-quoted multi-line fold",
        "k: \"hello\n  world\"\n",
        f,
        Some('\\'),
    );
    emit(
        "double-quoted escaped break",
        "k: \"aaa\\\n  bbb\"\n",
        f,
        Some('\\'),
    );
    root("root plain, col-0 continuation (1-byte fold)", "aaa\nbbb\n");

    // ---- Task 9: the seven cases with no fixture row yet ----

    // `k: ~` / `k: true` — non-string scalars. `Yaml::as_str()` is None for
    // both, so pass the event's value string explicitly: content provenance
    // means the decoded *scalar text*, not the resolved `Yaml`.
    {
        let src = "k: ~\n";
        let y = parse(src).unwrap();
        let v = y.get_hash_value("k").unwrap();
        emit_node("k: ~", v, src, false, None, Some("~"));
    }
    {
        let src = "k: true\n";
        let y = parse(src).unwrap();
        let v = y.get_hash_value("k").unwrap();
        emit_node("k: true", v, src, false, None, Some("true"));
    }

    // Quoted key — `key_span` has the identical defect as `value_span`, so
    // its content provenance must derive the same way.
    {
        let src = "'quoted key': v\n";
        let y = parse(src).unwrap();
        let entries = y.as_hash().unwrap();
        emit_node("quoted key", &entries[0].key, src, false, Some('\''), None);
    }

    // Flow collection — `k: ['a b', "c\td"]`. Each item is a scalar in its
    // own right; walk them individually rather than via `get_hash_value`,
    // which only reaches the mapping value's own node.
    {
        let src = "k: ['a b', \"c\\td\"]\n";
        let y = parse(src).unwrap();
        let v = y.get_hash_value("k").unwrap();
        let items = v.as_array().unwrap();
        emit_node(
            "flow collection item 0",
            &items[0],
            src,
            false,
            Some('\''),
            None,
        );
        emit_node(
            "flow collection item 1",
            &items[1],
            src,
            false,
            Some('\\'),
            None,
        );
    }

    // Tagged scalar — the node marker points at the VALUE, not the tag, so
    // the tag needs no extra arithmetic.
    emit("tagged scalar", "k: !path 'x'\n", f, Some('\''));

    // Plain double-quoted, no escapes — every other double-quoted fixture
    // has an escape in it.
    emit("double-quoted plain", "k: \"hello\"\n", f, Some('\\'));

    // `\n` as an escape.
    emit("double-quoted \\n", "k: \"a\\nb\"\n", f, Some('\\'));

    println!();
    hdr();
    emit("empty value", "k:\n", f, None);
    emit("empty single-quoted", "k: ''\n", f, Some('\''));
    emit("empty block scalar", "k: |\n", b, None);
    emit(
        "empty block scalar, next key follows",
        "k: |\nj: 1\n",
        b,
        None,
    );
}
