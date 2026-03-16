/*
 * c_shim.rs
 * Copyright (c) 2025 Posit, PBC
 */

use std::{
    alloc::{self, Layout},
    ffi::{c_char, c_int, c_void},
    mem::align_of,
    ptr,
};

type CDouble = f64;

/* -------------------------------- stdlib.h -------------------------------- */

#[no_mangle]
pub unsafe extern "C" fn abort() {
    panic!("Aborted from C");
}

#[no_mangle]
pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    if size == 0 {
        return ptr::null_mut();
    }

    let (layout, offset_to_data) = layout_for_size_prepended(size);
    let buf = alloc::alloc(layout);
    store_layout(buf, layout, offset_to_data)
}

#[no_mangle]
pub unsafe extern "C" fn calloc(count: usize, size: usize) -> *mut c_void {
    if count == 0 || size == 0 {
        return ptr::null_mut();
    }

    let (layout, offset_to_data) = layout_for_size_prepended(size * count);
    let buf = alloc::alloc_zeroed(layout);
    store_layout(buf, layout, offset_to_data)
}

#[no_mangle]
pub unsafe extern "C" fn realloc(buf: *mut c_void, new_size: usize) -> *mut c_void {
    if buf.is_null() {
        malloc(new_size)
    } else if new_size == 0 {
        free(buf);
        ptr::null_mut()
    } else {
        let (old_buf, old_layout) = retrieve_layout(buf);
        let (new_layout, offset_to_data) = layout_for_size_prepended(new_size);
        let new_buf = alloc::realloc(old_buf, old_layout, new_layout.size());
        store_layout(new_buf, new_layout, offset_to_data)
    }
}

#[no_mangle]
pub unsafe extern "C" fn free(buf: *mut c_void) {
    if buf.is_null() {
        return;
    }
    let (buf, layout) = retrieve_layout(buf);
    alloc::dealloc(buf, layout);
}

// In all these allocations, we store the layout before the data for later retrieval.
// This is because we need to know the layout when deallocating the memory.
// Here are some helper methods for that:

/// Given a pointer to the data, retrieve the layout and the pointer to the layout.
unsafe fn retrieve_layout(buf: *mut c_void) -> (*mut u8, Layout) {
    let (_, layout_offset) = Layout::new::<Layout>()
        .extend(Layout::from_size_align(0, align_of::<*const u8>() * 2).unwrap())
        .unwrap();

    let buf = (buf as *mut u8).offset(-(layout_offset as isize));
    let layout = *(buf as *mut Layout);

    (buf, layout)
}

/// Calculate a layout for a given size with space for storing a layout at the start.
/// Returns the layout and the offset to the data.
fn layout_for_size_prepended(size: usize) -> (Layout, usize) {
    Layout::new::<Layout>()
        .extend(Layout::from_size_align(size, align_of::<*const u8>() * 2).unwrap())
        .unwrap()
}

/// Store a layout in the pointer, returning a pointer to where the data should be stored.
unsafe fn store_layout(buf: *mut u8, layout: Layout, offset_to_data: usize) -> *mut c_void {
    *(buf as *mut Layout) = layout;
    (buf as *mut u8).offset(offset_to_data as isize) as *mut c_void
}

/* -------------------------------- string.h -------------------------------- */

#[no_mangle]
pub unsafe extern "C" fn memcpy(dest: *mut c_void, src: *const c_void, size: usize) -> *mut c_void {
    std::ptr::copy_nonoverlapping(src, dest, size);
    dest
}

#[no_mangle]
pub unsafe extern "C" fn memmove(
    dest: *mut c_void,
    src: *const c_void,
    size: usize,
) -> *mut c_void {
    std::ptr::copy(src, dest, size);
    dest
}

#[no_mangle]
pub unsafe extern "C" fn memset(s: *mut c_void, c: i32, n: usize) -> *mut c_void {
    let slice = std::slice::from_raw_parts_mut(s as *mut u8, n);
    slice.fill(c as u8);
    s
}

#[no_mangle]
pub unsafe extern "C" fn memcmp(ptr1: *const c_void, ptr2: *const c_void, n: usize) -> c_int {
    let s1 = std::slice::from_raw_parts(ptr1 as *const u8, n);
    let s2 = std::slice::from_raw_parts(ptr2 as *const u8, n);

    for (a, b) in s1.iter().zip(s2.iter()) {
        if *a != *b {
            return (*a as i32) - (*b as i32);
        }
    }

    0
}

#[no_mangle]
pub unsafe extern "C" fn strncmp(ptr1: *const c_void, ptr2: *const c_void, n: usize) -> c_int {
    let s1 = std::slice::from_raw_parts(ptr1 as *const u8, n);
    let s2 = std::slice::from_raw_parts(ptr2 as *const u8, n);

    for (a, b) in s1.iter().zip(s2.iter()) {
        if *a != *b || *a == 0 {
            return (*a as i32) - (*b as i32);
        }
    }

    0
}

/* -------------------------------- wctype.h -------------------------------- */

#[no_mangle]
pub unsafe extern "C" fn iswspace(c: c_int) -> bool {
    char::from_u32(c as u32).map_or(false, |c| c.is_whitespace())
}

#[no_mangle]
pub unsafe extern "C" fn iswalnum(c: c_int) -> bool {
    char::from_u32(c as u32).map_or(false, |c| c.is_alphanumeric())
}

#[no_mangle]
pub unsafe extern "C" fn iswdigit(c: c_int) -> bool {
    char::from_u32(c as u32).map_or(false, |c| c.is_digit(10))
}

#[no_mangle]
pub unsafe extern "C" fn iswalpha(c: c_int) -> bool {
    char::from_u32(c as u32).map_or(false, |c| c.is_alphabetic())
}

// Note: Not provided by https://github.com/cacticouncil/lilypad, but we needed
// this one too. We could contribute this back upstream? Note that
// `towlower()`'s C function docs say it is only guaranteed to work in 1:1
// mapping cases, so that is what we reimplement here as well.
// https://en.cppreference.com/w/c/string/wide/towlower
#[no_mangle]
pub unsafe extern "C" fn towlower(c: c_int) -> c_int {
    char::from_u32(c as u32).map_or(0, |c| {
        c.to_lowercase().next().map(|c| c as i32).unwrap_or(0)
    })
}

/* --------------------------------- time.h --------------------------------- */

#[no_mangle]
pub unsafe extern "C" fn clock() -> u32 {
    // clock_t is unsigned long, which is 32-bit on wasm32
    0
}

/* --------------------------------- ctype.h -------------------------------- */

#[no_mangle]
pub unsafe extern "C" fn isprint(c: c_int) -> bool {
    c >= 32 && c <= 126
}

/* --------------------------------- stdio.h -------------------------------- */

#[no_mangle]
pub unsafe extern "C" fn fprintf(_file: *mut c_void, _format: *const c_void, _args: ...) -> c_int {
    panic!("fprintf is not supported");
}

#[no_mangle]
pub unsafe extern "C" fn fputs(_s: *const c_void, _file: *mut c_void) -> c_int {
    panic!("fputs is not supported");
}

#[no_mangle]
pub unsafe extern "C" fn fputc(_c: c_int, _file: *mut c_void) -> c_int {
    panic!("fputc is not supported");
}

#[no_mangle]
pub unsafe extern "C" fn fdopen(_fd: c_int, _mode: *const c_void) -> *mut c_void {
    panic!("fdopen is not supported");
}

#[no_mangle]
pub unsafe extern "C" fn fclose(_file: *mut c_void) -> c_int {
    panic!("fclose is not supported");
}

#[no_mangle]
pub unsafe extern "C" fn fwrite(
    _ptr: *const c_void,
    _size: usize,
    _nmemb: usize,
    _stream: *mut c_void,
) -> usize {
    panic!("fwrite is not supported");
}

// Track if our snprintf is being called
static SNPRINTF_CALL_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Get snprintf call count for debugging
pub fn get_snprintf_call_count() -> usize {
    SNPRINTF_CALL_COUNT.load(std::sync::atomic::Ordering::SeqCst)
}

/// Minimal snprintf implementation for tree-sitter logging.
///
/// Tree-sitter uses snprintf to format log messages. This implementation
/// handles the common format specifiers used by tree-sitter's logging.
#[no_mangle]
pub unsafe extern "C" fn snprintf(
    buf: *mut c_char,
    size: usize,
    format: *const c_char,
    mut args: ...
) -> c_int {
    SNPRINTF_CALL_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    if buf.is_null() || size == 0 {
        return 0;
    }

    let format_str = std::ffi::CStr::from_ptr(format);
    let format_bytes = format_str.to_bytes();

    let mut output = Vec::with_capacity(size);
    let mut i = 0;

    while i < format_bytes.len() && output.len() < size - 1 {
        if format_bytes[i] == b'%' && i + 1 < format_bytes.len() {
            i += 1;
            // Skip flags, width, precision
            while i < format_bytes.len()
                && (format_bytes[i] == b'-'
                    || format_bytes[i] == b'+'
                    || format_bytes[i] == b' '
                    || format_bytes[i] == b'#'
                    || format_bytes[i] == b'0'
                    || format_bytes[i].is_ascii_digit()
                    || format_bytes[i] == b'.')
            {
                i += 1;
            }

            if i >= format_bytes.len() {
                break;
            }

            match format_bytes[i] {
                b'd' | b'i' => {
                    let val: c_int = args.arg();
                    let s = val.to_string();
                    for b in s.bytes() {
                        if output.len() < size - 1 {
                            output.push(b);
                        }
                    }
                }
                b'u' => {
                    let val: u32 = args.arg();
                    let s = val.to_string();
                    for b in s.bytes() {
                        if output.len() < size - 1 {
                            output.push(b);
                        }
                    }
                }
                b's' => {
                    let ptr: *const c_char = args.arg();
                    if !ptr.is_null() {
                        let cstr = std::ffi::CStr::from_ptr(ptr);
                        for b in cstr.to_bytes() {
                            if output.len() < size - 1 {
                                output.push(*b);
                            }
                        }
                    }
                }
                b'c' => {
                    let val: c_int = args.arg();
                    if output.len() < size - 1 {
                        output.push(val as u8);
                    }
                }
                b'%' => {
                    if output.len() < size - 1 {
                        output.push(b'%');
                    }
                }
                b'l' => {
                    // Handle %ld, %lu, %lld, %llu
                    i += 1;
                    if i < format_bytes.len() {
                        match format_bytes[i] {
                            b'd' | b'i' => {
                                let val: i64 = args.arg();
                                let s = val.to_string();
                                for b in s.bytes() {
                                    if output.len() < size - 1 {
                                        output.push(b);
                                    }
                                }
                            }
                            b'u' => {
                                let val: u64 = args.arg();
                                let s = val.to_string();
                                for b in s.bytes() {
                                    if output.len() < size - 1 {
                                        output.push(b);
                                    }
                                }
                            }
                            b'l' => {
                                // %lld or %llu
                                i += 1;
                                if i < format_bytes.len() {
                                    match format_bytes[i] {
                                        b'd' | b'i' => {
                                            let val: i64 = args.arg();
                                            let s = val.to_string();
                                            for b in s.bytes() {
                                                if output.len() < size - 1 {
                                                    output.push(b);
                                                }
                                            }
                                        }
                                        b'u' => {
                                            let val: u64 = args.arg();
                                            let s = val.to_string();
                                            for b in s.bytes() {
                                                if output.len() < size - 1 {
                                                    output.push(b);
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                b'z' => {
                    // Handle %zu, %zd (size_t)
                    i += 1;
                    if i < format_bytes.len() {
                        match format_bytes[i] {
                            b'u' => {
                                let val: usize = args.arg();
                                let s = val.to_string();
                                for b in s.bytes() {
                                    if output.len() < size - 1 {
                                        output.push(b);
                                    }
                                }
                            }
                            b'd' | b'i' => {
                                let val: isize = args.arg();
                                let s = val.to_string();
                                for b in s.bytes() {
                                    if output.len() < size - 1 {
                                        output.push(b);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {
                    // Unknown specifier, just skip
                }
            }
            i += 1;
        } else {
            if output.len() < size - 1 {
                output.push(format_bytes[i]);
            }
            i += 1;
        }
    }

    // Null-terminate
    let written = output.len();
    for (j, b) in output.into_iter().enumerate() {
        *buf.add(j) = b as c_char;
    }
    *buf.add(written) = 0;

    written as c_int
}

#[no_mangle]
pub unsafe extern "C" fn vsnprintf(
    _buf: *mut c_char,
    _size: usize,
    _format: *const c_char,
    _args: ...
) -> c_int {
    // vsnprintf with va_list is harder to implement; tree-sitter primarily uses snprintf
    0
}

/* ====================================================================== */
/*  Lua WASM support: catch_unwind/panic replacement for setjmp/longjmp   */
/* ====================================================================== */

/// Replacement for Lua's longjmp-based error throw.
/// Called from LUAI_THROW macro in ldo.c via luaD_throw.
#[no_mangle]
pub extern "C-unwind" fn rust_lua_throw() -> ! {
    panic!("lua error");
}

/// Replacement for Lua's setjmp-based protected call.
/// Called from LUAI_TRY macro in ldo.c via luaD_rawrunprotected.
#[no_mangle]
pub extern "C-unwind" fn rust_lua_protected_call(
    f: extern "C-unwind" fn(*mut c_void, *mut c_void),
    l: *mut c_void,
    ud: *mut c_void,
) -> i32 {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(l, ud))) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

/* ====================================================================== */
/*  Lua library stubs — libraries we skip but linit.c still references    */
/* ====================================================================== */

#[no_mangle]
pub unsafe extern "C" fn luaopen_io(_l: *mut c_void) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn luaopen_os(_l: *mut c_void) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn luaopen_package(_l: *mut c_void) -> c_int {
    0
}

/* ====================================================================== */
/*  string.h additions for Lua                                            */
/* ====================================================================== */

#[no_mangle]
pub unsafe extern "C" fn strlen(s: *const c_char) -> usize {
    let mut len = 0;
    while *s.add(len) != 0 {
        len += 1;
    }
    len
}

#[no_mangle]
pub unsafe extern "C" fn strcmp(s1: *const c_char, s2: *const c_char) -> c_int {
    let mut i = 0;
    loop {
        let a = *s1.add(i) as u8;
        let b = *s2.add(i) as u8;
        if a != b {
            return (a as c_int) - (b as c_int);
        }
        if a == 0 {
            return 0;
        }
        i += 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn strchr(s: *const c_char, c: c_int) -> *mut c_char {
    let c = c as u8;
    let mut p = s;
    loop {
        if *p as u8 == c {
            return p as *mut c_char;
        }
        if *p == 0 {
            return ptr::null_mut();
        }
        p = p.add(1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn strcpy(dest: *mut c_char, src: *const c_char) -> *mut c_char {
    let mut i = 0;
    loop {
        *dest.add(i) = *src.add(i);
        if *src.add(i) == 0 {
            break;
        }
        i += 1;
    }
    dest
}

#[no_mangle]
pub unsafe extern "C" fn memchr(s: *const c_void, c: c_int, n: usize) -> *mut c_void {
    let s = s as *const u8;
    let c = c as u8;
    for i in 0..n {
        if *s.add(i) == c {
            return s.add(i) as *mut c_void;
        }
    }
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn strpbrk(s: *const c_char, accept: *const c_char) -> *mut c_char {
    let mut p = s;
    while *p != 0 {
        let mut a = accept;
        while *a != 0 {
            if *p == *a {
                return p as *mut c_char;
            }
            a = a.add(1);
        }
        p = p.add(1);
    }
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn strspn(s: *const c_char, accept: *const c_char) -> usize {
    let mut count = 0;
    let mut p = s;
    while *p != 0 {
        let mut found = false;
        let mut a = accept;
        while *a != 0 {
            if *p == *a {
                found = true;
                break;
            }
            a = a.add(1);
        }
        if !found {
            break;
        }
        count += 1;
        p = p.add(1);
    }
    count
}

#[no_mangle]
pub unsafe extern "C" fn strcoll(s1: *const c_char, s2: *const c_char) -> c_int {
    // No locale support in WASM — fall back to strcmp
    strcmp(s1, s2)
}

#[no_mangle]
pub unsafe extern "C" fn strerror(_errnum: c_int) -> *const c_char {
    b"unknown error\0".as_ptr() as *const c_char
}

/* ====================================================================== */
/*  ctype.h additions for Lua                                             */
/* ====================================================================== */

#[no_mangle]
pub unsafe extern "C" fn isdigit(c: c_int) -> c_int {
    ((c as u8) >= b'0' && (c as u8) <= b'9') as c_int
}

#[no_mangle]
pub unsafe extern "C" fn isalpha(c: c_int) -> c_int {
    let b = c as u8;
    ((b >= b'A' && b <= b'Z') || (b >= b'a' && b <= b'z')) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn isalnum(c: c_int) -> c_int {
    (isdigit(c) != 0 || isalpha(c) != 0) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn isspace(c: c_int) -> c_int {
    matches!(c as u8, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn isupper(c: c_int) -> c_int {
    ((c as u8) >= b'A' && (c as u8) <= b'Z') as c_int
}

#[no_mangle]
pub unsafe extern "C" fn islower(c: c_int) -> c_int {
    ((c as u8) >= b'a' && (c as u8) <= b'z') as c_int
}

#[no_mangle]
pub unsafe extern "C" fn iscntrl(c: c_int) -> c_int {
    ((c as u8) < 0x20 || (c as u8) == 0x7f) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn ispunct(c: c_int) -> c_int {
    (isprint(c) && isalnum(c) == 0 && isspace(c) == 0) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn isgraph(c: c_int) -> c_int {
    ((c as u8) > 0x20 && (c as u8) < 0x7f) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn isxdigit(c: c_int) -> c_int {
    let b = c as u8;
    ((b >= b'0' && b <= b'9') || (b >= b'A' && b <= b'F') || (b >= b'a' && b <= b'f')) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn toupper(c: c_int) -> c_int {
    if islower(c) != 0 { c - 32 } else { c }
}

#[no_mangle]
pub unsafe extern "C" fn tolower(c: c_int) -> c_int {
    if isupper(c) != 0 { c + 32 } else { c }
}

/* ====================================================================== */
/*  stdlib.h additions for Lua                                            */
/* ====================================================================== */

#[no_mangle]
pub unsafe extern "C" fn abs(x: c_int) -> c_int {
    if x < 0 { -x } else { x }
}

#[no_mangle]
pub unsafe extern "C" fn strtod(s: *const c_char, endptr: *mut *mut c_char) -> CDouble {
    let mut p = s;
    // Skip whitespace
    while isspace(*p as c_int) != 0 {
        p = p.add(1);
    }

    let neg = if *p as u8 == b'-' {
        p = p.add(1);
        true
    } else {
        if *p as u8 == b'+' {
            p = p.add(1);
        }
        false
    };

    // Check for hex float (0x...)
    let is_hex = *p as u8 == b'0' && (*p.add(1) as u8 == b'x' || *p.add(1) as u8 == b'X');

    let mut result: f64;
    let start = p;

    if is_hex {
        p = p.add(2); // skip "0x"
        result = 0.0;
        let mut has_digits = false;

        // Integer part
        while isxdigit(*p as c_int) != 0 {
            has_digits = true;
            let digit = hex_digit(*p as u8);
            result = result * 16.0 + digit as f64;
            p = p.add(1);
        }

        // Fractional part
        if *p as u8 == b'.' {
            p = p.add(1);
            let mut frac = 1.0 / 16.0;
            while isxdigit(*p as c_int) != 0 {
                has_digits = true;
                let digit = hex_digit(*p as u8);
                result += digit as f64 * frac;
                frac /= 16.0;
                p = p.add(1);
            }
        }

        if !has_digits {
            // No valid digits after 0x
            if !endptr.is_null() {
                *endptr = s as *mut c_char;
            }
            return 0.0;
        }

        // Exponent (p/P)
        if *p as u8 == b'p' || *p as u8 == b'P' {
            p = p.add(1);
            let exp_neg = if *p as u8 == b'-' {
                p = p.add(1);
                true
            } else {
                if *p as u8 == b'+' {
                    p = p.add(1);
                }
                false
            };
            let mut exp: i32 = 0;
            while isdigit(*p as c_int) != 0 {
                exp = exp * 10 + (*p as u8 - b'0') as i32;
                p = p.add(1);
            }
            if exp_neg {
                exp = -exp;
            }
            result *= f64::powi(2.0, exp);
        }
    } else {
        // Decimal float
        result = 0.0;
        let mut has_digits = false;

        // Check for inf/nan
        let check = [*p as u8, *p.add(1) as u8, *p.add(2) as u8];
        if check == [b'i', b'n', b'f'] || check == [b'I', b'N', b'F'] {
            p = p.add(3);
            // Check for "infinity"
            let rest = [
                *p as u8,
                *p.add(1) as u8,
                *p.add(2) as u8,
                *p.add(3) as u8,
                *p.add(4) as u8,
            ];
            if rest == [b'i', b'n', b'i', b't', b'y'] || rest == [b'I', b'N', b'I', b'T', b'Y'] {
                p = p.add(5);
            }
            result = f64::INFINITY;
            if neg {
                result = -result;
            }
            if !endptr.is_null() {
                *endptr = p as *mut c_char;
            }
            return result;
        }
        if check == [b'n', b'a', b'n'] || check == [b'N', b'A', b'N'] {
            p = p.add(3);
            if !endptr.is_null() {
                *endptr = p as *mut c_char;
            }
            return f64::NAN;
        }

        // Integer part
        while isdigit(*p as c_int) != 0 {
            has_digits = true;
            result = result * 10.0 + (*p as u8 - b'0') as f64;
            p = p.add(1);
        }

        // Fractional part
        if *p as u8 == b'.' {
            p = p.add(1);
            let mut frac = 0.1;
            while isdigit(*p as c_int) != 0 {
                has_digits = true;
                result += (*p as u8 - b'0') as f64 * frac;
                frac *= 0.1;
                p = p.add(1);
            }
        }

        if !has_digits {
            if !endptr.is_null() {
                *endptr = s as *mut c_char;
            }
            return 0.0;
        }

        // Exponent (e/E)
        if *p as u8 == b'e' || *p as u8 == b'E' {
            p = p.add(1);
            let exp_neg = if *p as u8 == b'-' {
                p = p.add(1);
                true
            } else {
                if *p as u8 == b'+' {
                    p = p.add(1);
                }
                false
            };
            let mut exp: i32 = 0;
            while isdigit(*p as c_int) != 0 {
                exp = exp * 10 + (*p as u8 - b'0') as i32;
                p = p.add(1);
            }
            if exp_neg {
                exp = -exp;
            }
            result *= f64::powi(10.0, exp);
        }
    }

    if neg {
        result = -result;
    }
    if !endptr.is_null() {
        *endptr = p as *mut c_char;
    }
    result
}

fn hex_digit(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

/* ====================================================================== */
/*  math.h — Lua math library needs these                                 */
/* ====================================================================== */

#[no_mangle]
pub unsafe extern "C" fn frexp(x: CDouble, exp: *mut c_int) -> CDouble {
    if x == 0.0 {
        *exp = 0;
        return 0.0;
    }
    let bits = x.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32 - 1022;
    *exp = exponent;
    let mantissa_bits = (bits & 0x800f_ffff_ffff_ffff) | 0x3fe0_0000_0000_0000;
    f64::from_bits(mantissa_bits)
}

/* ====================================================================== */
/*  locale.h — Lua uses localeconv for decimal point detection            */
/* ====================================================================== */

#[repr(C)]
pub struct Lconv {
    pub decimal_point: *const c_char,
}

unsafe impl Sync for Lconv {}

static LCONV: Lconv = Lconv {
    decimal_point: b".\0".as_ptr() as *const c_char,
};

#[no_mangle]
pub unsafe extern "C" fn localeconv() -> *const Lconv {
    &LCONV
}

/* ====================================================================== */
/*  errno.h                                                               */
/* ====================================================================== */

static mut ERRNO_VALUE: c_int = 0;

#[no_mangle]
pub unsafe extern "C" fn __errno_location() -> *mut c_int {
    &raw mut ERRNO_VALUE
}

/* ====================================================================== */
/*  time.h — Lua needs time() for math.randomseed default                 */
/* ====================================================================== */

#[no_mangle]
pub unsafe extern "C" fn time(_t: *mut c_void) -> i32 {
    // time_t is long, which is 32-bit on wasm32.
    // Return a pseudo-timestamp — only used for default random seed.
    42
}

/* ====================================================================== */
/*  stdio.h additions — Lua's lauxlib.c needs some file operations        */
/* ====================================================================== */

#[no_mangle]
pub unsafe extern "C" fn fopen(_path: *const c_char, _mode: *const c_char) -> *mut c_void {
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn freopen(
    _path: *const c_char,
    _mode: *const c_char,
    _stream: *mut c_void,
) -> *mut c_void {
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn fgets(
    _buf: *mut c_char,
    _size: c_int,
    _stream: *mut c_void,
) -> *mut c_char {
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn fread(
    _ptr: *mut c_void,
    _size: usize,
    _nmemb: usize,
    _stream: *mut c_void,
) -> usize {
    0
}

#[no_mangle]
pub unsafe extern "C" fn fflush(_stream: *mut c_void) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn ferror(_stream: *mut c_void) -> c_int {
    0
}

#[no_mangle]
pub unsafe extern "C" fn feof(_stream: *mut c_void) -> c_int {
    1
}

#[no_mangle]
pub unsafe extern "C" fn getc(_stream: *mut c_void) -> c_int {
    -1 // EOF
}
