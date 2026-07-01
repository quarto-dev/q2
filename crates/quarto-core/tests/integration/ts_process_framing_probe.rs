/*
 * tests/integration/ts_process_framing_probe.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Bug C diagnosis probe (plan 2026-07-02-preview-capture-delivery.md, seam
 * "PC-C" candidate). NOT a fix — this is the P0 evidence harness that pins the
 * engine-host stdout READER's framing behavior at the `StdioReadHalf::recv`
 * boundary under the two live-session suspects:
 *
 *   (a) a very large single-line frame (multi-MB base64 executeResult) —
 *       suspect: the reader truncates / mis-splits a frame bigger than the
 *       BufReader's internal buffer, rejecting a legitimate frame.
 *   (b) a foreign (non-JSON) line interleaved onto the wire — suspect: a child
 *       process's stdout (an ANSI julia server log line) leaking onto the Deno
 *       host's stdout fd corrupts framing.
 *
 * These probes exercise the REAL `spawn_into` + `StdioReadHalf::recv` path (the
 * exact reader the demux thread runs), driven by a `deno eval` child that emits
 * precise bytes on stdout. Deno-gated (early-return skip when `deno` is not on
 * PATH), matching `echo_engine_e2e.rs` / `julia_engine_e2e.rs`.
 *
 * The amplification consequence — a single `RecvError::Malformed` making
 * `reader_loop` (ts_process.rs:930-954) set `shutting_down`, broadcast an error
 * to EVERY pending slot, and kill the whole Deno subprocess — is not re-executed
 * here (the demux `reader_loop` + `with_transport` harness is `#[cfg(test)]`,
 * unit-tier only). It is proven by reading that arm; this probe pins the framing
 * decision (Ok vs Malformed) that feeds it.
 */

#![cfg(not(target_arch = "wasm32"))]

use std::process::Command;
use std::sync::{Arc, Mutex};

use quarto_core::engine::ts_process::{EngineReadHalf, RecvError, spawn_into};
use quarto_core::engine::ts_protocol::FromEngine;

/// Return `true` when `deno` is on PATH (the byte-emitter for these probes).
fn deno_available() -> bool {
    Command::new("deno")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Spawn `deno eval <script>` and return the wired read half. The child's
/// stdout is the protocol channel the reader parses.
fn spawn_deno_eval(script: &str) -> quarto_core::engine::ts_process::StdioReadHalf {
    let mut cmd = Command::new("deno");
    cmd.arg("eval").arg(script);
    let child_slot = Arc::new(Mutex::new(None));
    let (_write, read, _stderr) = spawn_into(cmd, child_slot).expect("spawn deno eval");
    read
}

// ── PC-C(a): a >1 MB single-line frame parses cleanly (large-frame suspect
// RULED OUT) ───────────────────────────────────────────────────────────────
//
// `BufRead::read_line` has NO size cap — it loops over the BufReader's (8 KB)
// internal buffer, growing the String until it hits `\n` or EOF. So a
// legitimate multi-MB executeResult frame terminated by a single `\n` must
// round-trip to `Ok(Response)`. If this probe were RED it would incriminate the
// reader for the "legit executeResult frame rejected" symptom; it is GREEN,
// which is the evidence that shifts blame to the interleave suspect (b).
#[test]
fn pc_c_a_large_single_line_frame_parses() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — pc_c_a_large_single_line_frame_parses");
        return;
    }
    // ~2 MB payload inside a valid FromEngine::Error frame, then one newline.
    let script = r#"
const big = "x".repeat(2_000_000);
const frame = JSON.stringify({ id: 7, msg: { type: "error", message: big, stack: null } });
Deno.stdout.writeSync(new TextEncoder().encode(frame + "\n"));
"#;
    let mut read = spawn_deno_eval(script);
    match read.recv() {
        Ok(resp) => {
            assert_eq!(resp.id, 7, "id must round-trip on a large frame");
            match resp.msg {
                FromEngine::Error { message, .. } => {
                    assert_eq!(
                        message.len(),
                        2_000_000,
                        "the full >1MB single-line frame must survive read_line intact \
                         (no truncation / mis-split); got {} bytes",
                        message.len()
                    );
                }
                other => panic!("expected FromEngine::Error, got {other:?}"),
            }
        }
        Err(e) => panic!(
            "a legitimate >1MB single-line frame must parse to Ok(Response); \
             the reader instead returned {e:?} — this would incriminate the reader \
             for the 'legit executeResult rejected' symptom"
        ),
    }
}

// ── PC-C(b): a foreign non-JSON line on the wire → RecvError::Malformed (the
// stdout-inheritance corruption path) ──────────────────────────────────────
//
// Models symptom #1 from the live session: an ANSI-colored julia server log
// line (`[ Info: Log started at …`) arriving on the engine-host's stdout fd
// because a child process's stdout was inherited/leaked onto the Deno host's
// wire channel. The reader has no way to distinguish it from a protocol frame,
// so it returns `Malformed` — which upstream (`reader_loop`) escalates to a
// whole-subprocess kill + broadcast-to-all-pending. This probe pins the framing
// verdict; the escalation is at ts_process.rs:930-954.
#[test]
fn pc_c_b_foreign_line_is_malformed() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — pc_c_b_foreign_line_is_malformed");
        return;
    }
    // A stray ANSI julia log line (its own `\n`), then a legitimate frame.
    let script = r#"
const enc = new TextEncoder();
const stray = "[36m[ Info: Log started at 2026-07-02T13:11:20.379[0m\n";
const frame = JSON.stringify({ id: 3, msg: { type: "error", message: "real", stack: null } }) + "\n";
Deno.stdout.writeSync(enc.encode(stray + frame));
"#;
    let mut read = spawn_deno_eval(script);

    // First line is the foreign log line → Malformed carrying the raw bytes.
    match read.recv() {
        Err(RecvError::Malformed(line)) => {
            assert!(
                line.contains("Log started at"),
                "the malformed payload must be the leaked julia log line; got: {line:?}"
            );
        }
        other => panic!(
            "a foreign non-JSON line on the wire must return RecvError::Malformed; got {other:?}"
        ),
    }

    // The legitimate frame that FOLLOWED the stray line is still on the wire
    // here (recv() surfaces it on the next call), but in the real demux the
    // Malformed above already triggered the whole-subprocess kill — so this
    // trailing frame would never be delivered in production. Reading it here
    // documents that the bytes were valid; the loss is structural.
    match read.recv() {
        Ok(resp) => assert_eq!(resp.id, 3, "the trailing frame itself was well-formed"),
        Err(RecvError::Eof) => { /* acceptable: child may have closed */ }
        other => panic!("unexpected trailing recv result: {other:?}"),
    }
}

// ── PC-C(b'): a frame with foreign bytes spliced INTO its middle → Malformed
// (symptom #2: the legit executeResult frame corrupted mid-flight) ──────────
//
// Models symptom #2: a genuine executeResult frame that arrived corrupted
// because foreign bytes (a concurrent writer on the same fd) landed inside it.
// Splicing a raw `\n` + noise into the JSON both breaks the JSON AND splits the
// logical frame across two physical lines → the first `read_line` yields a
// truncated, unparseable prefix → Malformed. This is why "sometimes no result
// appears": the executeResult is silently dropped.
#[test]
fn pc_c_b_prime_interleaved_bytes_corrupt_frame() {
    if !deno_available() {
        eprintln!("SKIP: deno not on PATH — pc_c_b_prime_interleaved_bytes_corrupt_frame");
        return;
    }
    // Build a valid frame, then splice a stray newline + log noise into its
    // middle to simulate a concurrent fd write mid-frame.
    let script = r#"
const enc = new TextEncoder();
const valid = JSON.stringify({ id: 5, msg: { type: "error", message: "AAAABBBB", stack: null } });
const cut = Math.floor(valid.length / 2);
const corrupted =
  valid.slice(0, cut) + "\n[ Info: interleaved worker log\n" + valid.slice(cut) + "\n";
Deno.stdout.writeSync(enc.encode(corrupted));
"#;
    let mut read = spawn_deno_eval(script);
    match read.recv() {
        Err(RecvError::Malformed(line)) => {
            assert!(
                !line.is_empty(),
                "the truncated frame prefix must be surfaced as Malformed"
            );
        }
        other => panic!(
            "a frame with foreign bytes spliced into its middle must fail framing \
             (Malformed); got {other:?}"
        ),
    }
}
