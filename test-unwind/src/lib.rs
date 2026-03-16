use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn test_catch_unwind() -> String {
    match std::panic::catch_unwind(|| "no panic".to_string()) {
        Ok(s) => format!("OK: {s}"),
        Err(_) => "caught panic".to_string(),
    }
}

#[wasm_bindgen]
pub fn test_catch_panic() -> String {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        panic!("test panic");
    })) {
        Ok(()) => "no panic (unexpected)".to_string(),
        Err(_) => "caught panic successfully".to_string(),
    }
}

#[wasm_bindgen]
pub fn test_catch_through_c_unwind() -> String {
    // Simulate what Lua does: call through extern "C-unwind" function
    extern "C-unwind" fn inner_throw() -> ! {
        panic!("lua-like error");
    }

    extern "C-unwind" fn protected_call(f: extern "C-unwind" fn() -> !) -> i32 {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            f();
        })) {
            Ok(()) => 0,
            Err(_) => 1,
        }
    }

    let result = protected_call(inner_throw);
    if result == 1 {
        "caught C-unwind panic successfully".to_string()
    } else {
        "panic was not caught (unexpected)".to_string()
    }
}
