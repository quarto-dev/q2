/*
 * test_lua_popen_parallel.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `io.popen` must not deadlock when Lua filters run on parallel threads
 * (GH #1205, bd-hf93wq9r).
 *
 * Stock Lua defines `l_popen` as `(fflush(NULL), popen(c, m))` on POSIX.
 * On macOS, `fflush(NULL)` walks every open FILE without a lock and locks
 * each one, while a concurrent `fclose`/`pclose` on another thread destroys
 * that FILE's mutex. The walker then waits forever. Project renders run
 * filters on parallel workers, so a site whose extensions shell out
 * through `io.popen` hung at 0% CPU. The fix lives in the lua-src fork's
 * build script (`crates/lua-src-wasm/src/lib.rs`), which defines `l_popen`
 * without the flush. This test drives the real filter path on several
 * threads, mixing `io.popen` with plain `io.open`/`close` (the other side
 * of the race), and fails if they do not all finish in time.
 */

#![cfg(feature = "lua-filter")]

use pampa::lua::apply_lua_filters;
use pampa::pandoc::Pandoc;
use pampa::pandoc::ast_context::ASTContext;
use std::io::Write;
use std::sync::mpsc;
use std::time::Duration;

const THREADS: usize = 8;
const ITERATIONS: usize = 150;
/// Without the fix this hangs within a second or two on macOS; with it the
/// whole test takes a few seconds. The deadline leaves room for slow CI.
const DEADLINE: Duration = Duration::from_secs(120);

fn filter_source(scratch: &std::path::Path) -> String {
    // Lua long-bracket string, so the path needs no escaping.
    let scratch = scratch.display();
    format!(
        r#"
function Pandoc(doc)
  for i = 1, {ITERATIONS} do
    local h = assert(io.popen("echo hi"))
    local out = h:read("a")
    local ok, how, code = h:close()
    assert(out:match("^hi"), "unexpected popen output: " .. tostring(out))
    assert(ok and how == "exit" and code == 0,
      "unexpected close result: " .. tostring(ok) .. " " .. tostring(how) .. " " .. tostring(code))
    local f = assert(io.open([==[{scratch}]==], "w"))
    f:write("x")
    f:close()
  end
  return doc
end
"#
    )
}

fn run_one_filter(index: usize) -> Result<(), String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let filter_path = dir.path().join(format!("popen-{index}.lua"));
    let scratch = dir.path().join(format!("scratch-{index}.txt"));
    std::fs::File::create(&filter_path)
        .and_then(|mut f| f.write_all(filter_source(&scratch).as_bytes()))
        .map_err(|e| e.to_string())?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let system: std::sync::Arc<dyn quarto_system_runtime::SystemRuntime> =
        std::sync::Arc::new(quarto_system_runtime::NativeRuntime::new());
    runtime
        .block_on(apply_lua_filters(
            Pandoc::default(),
            ASTContext::anonymous(),
            &[filter_path],
            "html",
            system,
            None,
        ))
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn io_popen_does_not_deadlock_across_parallel_filter_threads() {
    let (tx, rx) = mpsc::channel();
    for index in 0..THREADS {
        let tx = tx.clone();
        std::thread::Builder::new()
            .name(format!("popen-filter-{index}"))
            .spawn(move || {
                let _ = tx.send((index, run_one_filter(index)));
            })
            .expect("spawn filter thread");
    }
    drop(tx);

    for _ in 0..THREADS {
        match rx.recv_timeout(DEADLINE) {
            Ok((index, Ok(()))) => {
                let _ = index;
            }
            Ok((index, Err(err))) => panic!("filter thread {index} failed: {err}"),
            // A deadlocked thread never returns; the panic ends the test
            // process, so the stuck threads do not keep it alive.
            Err(_) => panic!(
                "io.popen filters on {THREADS} parallel threads did not finish within \
                 {DEADLINE:?}: deadlock (GH #1205)"
            ),
        }
    }
}
