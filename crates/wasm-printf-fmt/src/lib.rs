//! printf-family format rendering, factored out of
//! `wasm-quarto-hub-client`'s `c_shim.rs` so it can be unit-tested on
//! native targets.
//!
//! The c_shim layer provides C-callable `snprintf`/`vsnprintf` on
//! `wasm32-unknown-unknown`. Those functions are thin wrappers: they
//! accept a `VaList`, parse the format string with [`parse_spec`],
//! dispatch each `%`-directive to [`render_one`] via a trait
//! [`VaArgSource`] that abstracts over the variadic argument source,
//! and finally copy the rendered bytes into the caller's buffer with
//! null-termination.
//!
//! See `claude-notes/plans/2026-04-20-wasm-shim-merge.md` for the
//! design rationale. Short version: this is the single source of truth
//! for printf formatting in the WASM binary — the upstream
//! `tree-sitter-language` stubs are neutralized via a local
//! `[patch.crates-io]` fork.
//!
//! Supported specifiers: `%d %i %u %s %c %% %x %X %p %o %ld %lu %lld
//! %llu %zu %zd %g %G %Lg`. Flags: `-`, `+`, space, `#`, `0`. Field
//! width and precision are supported on all integer / string / float
//! conversions.
//!
//! This crate uses `std` (not `no_std`) because `%g` formatting needs
//! `f64::log10`, which lives in `std::f64` and is not available in
//! `core`. The formatter is a compile-time-negligible dependency in
//! our WASM binary.

use std::ffi::{c_char, c_void};

/// Parsed `%`-spec.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatSpec {
    pub left_justify: bool, // '-'
    pub show_sign: bool,    // '+'
    pub space_prefix: bool, // ' '
    pub alternate: bool,    // '#'
    pub zero_pad: bool,     // '0'
    pub width: Option<usize>,
    pub precision: Option<usize>,
    pub length: LengthMod,
}

/// C length-modifier: `l`, `ll`, `z`, or `L`.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthMod {
    #[default]
    None,
    L,    // 'l'
    LL,   // 'll'
    Z,    // 'z' (size_t)
    BigL, // 'L' (long double — treated as f64 on wasm32)
}

/// Advance past a format spec starting after the `%`. Returns
/// `(parsed_spec, conversion_byte, index_pointing_at_conversion_byte)`.
///
/// The caller should advance to `index + 1` to continue parsing after
/// consuming the spec. If the conversion byte is 0, the format ran off
/// the end before reaching a conversion — the caller should treat it as
/// malformed and stop.
pub fn parse_spec(fmt: &[u8], start: usize) -> (FormatSpec, u8, usize) {
    let mut spec = FormatSpec::default();
    let mut i = start;

    while i < fmt.len() {
        match fmt[i] {
            b'-' => spec.left_justify = true,
            b'+' => spec.show_sign = true,
            b' ' => spec.space_prefix = true,
            b'#' => spec.alternate = true,
            b'0' => spec.zero_pad = true,
            _ => break,
        }
        i += 1;
    }

    if i < fmt.len() && fmt[i].is_ascii_digit() {
        let mut w = 0usize;
        while i < fmt.len() && fmt[i].is_ascii_digit() {
            w = w * 10 + (fmt[i] - b'0') as usize;
            i += 1;
        }
        spec.width = Some(w);
    }

    if i < fmt.len() && fmt[i] == b'.' {
        i += 1;
        let mut p = 0usize;
        while i < fmt.len() && fmt[i].is_ascii_digit() {
            p = p * 10 + (fmt[i] - b'0') as usize;
            i += 1;
        }
        spec.precision = Some(p);
    }

    if i < fmt.len() {
        match fmt[i] {
            b'l' => {
                i += 1;
                if i < fmt.len() && fmt[i] == b'l' {
                    spec.length = LengthMod::LL;
                    i += 1;
                } else {
                    spec.length = LengthMod::L;
                }
            }
            b'z' => {
                spec.length = LengthMod::Z;
                i += 1;
            }
            b'L' => {
                spec.length = LengthMod::BigL;
                i += 1;
            }
            _ => {}
        }
    }

    let conv = if i < fmt.len() { fmt[i] } else { 0 };
    (spec, conv, i)
}

/// Pad a rendered piece into the output buffer with width / fill /
/// flags applied. Buffer-size budget (`size - 1` for the null
/// terminator) is enforced.
pub fn write_padded(out: &mut Vec<u8>, size: usize, rendered: &[u8], spec: &FormatSpec) {
    let width = spec.width.unwrap_or(0);
    let pad = width.saturating_sub(rendered.len());
    // Zero-pad is ignored when left-justifying (C standard).
    let fill = if spec.zero_pad && !spec.left_justify {
        b'0'
    } else {
        b' '
    };

    if !spec.left_justify {
        for _ in 0..pad {
            if out.len() < size.saturating_sub(1) {
                out.push(fill);
            }
        }
    }
    for &b in rendered {
        if out.len() < size.saturating_sub(1) {
            out.push(b);
        }
    }
    if spec.left_justify {
        for _ in 0..pad {
            if out.len() < size.saturating_sub(1) {
                out.push(b' ');
            }
        }
    }
}

/// Render a signed integer applying precision (min digits) and sign
/// flags. Does not handle width / left-justify — that's `write_padded`'s
/// job.
pub fn render_signed(buf: &mut Vec<u8>, value: i64, spec: &FormatSpec) {
    let neg = value < 0;
    let abs = if neg {
        (value as i128).unsigned_abs()
    } else {
        value as u128
    };
    let mut digits: Vec<u8> = Vec::with_capacity(20);
    let mut n = abs;
    if n == 0 {
        digits.push(b'0');
    } else {
        while n > 0 {
            digits.push(b'0' + (n % 10) as u8);
            n /= 10;
        }
    }
    digits.reverse();
    let precision = spec.precision.unwrap_or(0);
    while digits.len() < precision {
        digits.insert(0, b'0');
    }
    if neg {
        buf.push(b'-');
    } else if spec.show_sign {
        buf.push(b'+');
    } else if spec.space_prefix {
        buf.push(b' ');
    }
    buf.extend_from_slice(&digits);
}

/// Render an unsigned integer in base 10 with precision applied.
pub fn render_unsigned(buf: &mut Vec<u8>, value: u128, spec: &FormatSpec) {
    let mut digits: Vec<u8> = Vec::with_capacity(20);
    let mut n = value;
    if n == 0 {
        digits.push(b'0');
    } else {
        while n > 0 {
            digits.push(b'0' + (n % 10) as u8);
            n /= 10;
        }
    }
    digits.reverse();
    let precision = spec.precision.unwrap_or(0);
    while digits.len() < precision {
        digits.insert(0, b'0');
    }
    buf.extend_from_slice(&digits);
}

/// Render an unsigned integer in base 16. `uppercase` picks digit
/// casing. `spec.alternate` prepends `0x` / `0X` (C semantics: only
/// when value is non-zero).
pub fn render_hex(buf: &mut Vec<u8>, value: u128, spec: &FormatSpec, uppercase: bool) {
    let digits_tab: &[u8] = if uppercase {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut digits: Vec<u8> = Vec::with_capacity(16);
    let mut n = value;
    if n == 0 {
        digits.push(b'0');
    } else {
        while n > 0 {
            digits.push(digits_tab[(n & 0xF) as usize]);
            n >>= 4;
        }
    }
    digits.reverse();
    let precision = spec.precision.unwrap_or(0);
    while digits.len() < precision {
        digits.insert(0, b'0');
    }
    if spec.alternate && value != 0 {
        buf.push(b'0');
        buf.push(if uppercase { b'X' } else { b'x' });
    }
    buf.extend_from_slice(&digits);
}

/// Render a pointer as `0x<hex>`.
pub fn render_ptr(buf: &mut Vec<u8>, value: usize) {
    buf.extend_from_slice(b"0x");
    if value == 0 {
        buf.push(b'0');
    } else {
        let digits_tab = b"0123456789abcdef";
        let mut digits: Vec<u8> = Vec::with_capacity(16);
        let mut n = value;
        while n > 0 {
            digits.push(digits_tab[n & 0xF]);
            n >>= 4;
        }
        digits.reverse();
        buf.extend_from_slice(&digits);
    }
}

/// Render a double using `%g` semantics. Approximates the C standard:
/// pick between `%e` and `%f` based on exponent magnitude, honor the
/// precision flag as *significant digits* (default 6), strip trailing
/// zeros unless the `#` flag is set.
///
/// Not bit-for-bit identical with glibc — we rely on Rust's float
/// formatter — but matches the shape Lua's `tostring`/`string.format`
/// callers expect.
pub fn render_g(buf: &mut Vec<u8>, value: f64, spec: &FormatSpec) {
    if value.is_nan() {
        buf.extend_from_slice(b"nan");
        return;
    }
    if value.is_infinite() {
        if value < 0.0 {
            buf.push(b'-');
        } else if spec.show_sign {
            buf.push(b'+');
        } else if spec.space_prefix {
            buf.push(b' ');
        }
        buf.extend_from_slice(b"inf");
        return;
    }
    if value < 0.0 {
        buf.push(b'-');
    } else if spec.show_sign {
        buf.push(b'+');
    } else if spec.space_prefix {
        buf.push(b' ');
    }
    let abs = value.abs();
    let precision = spec.precision.unwrap_or(6).max(1);
    let rendered = if abs != 0.0 {
        let exp10 = abs.log10().floor() as i32;
        if exp10 < -4 || exp10 >= precision as i32 {
            let formatted = format!("{:.*e}", precision.saturating_sub(1), abs);
            strip_trailing_zeros_g(&formatted, spec.alternate)
        } else {
            let frac = (precision as i32 - 1 - exp10).max(0) as usize;
            let formatted = format!("{:.*}", frac, abs);
            strip_trailing_zeros_g(&formatted, spec.alternate)
        }
    } else {
        "0".to_string()
    };
    buf.extend_from_slice(rendered.as_bytes());
}

fn strip_trailing_zeros_g(s: &str, alternate: bool) -> String {
    if alternate {
        return s.to_string();
    }
    if let Some(e_idx) = s.find(['e', 'E']) {
        let (mantissa, exp) = s.split_at(e_idx);
        let trimmed = trim_mantissa(mantissa);
        return format!("{}{}", trimmed, exp);
    }
    trim_mantissa(s)
}

fn trim_mantissa(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let trimmed = s.trim_end_matches('0');
    let final_str = trimmed.trim_end_matches('.');
    final_str.to_string()
}

/// Trait abstracting over the two variadic-argument sources
/// (`VaList<'_>` from `extern "C" fn(…, args: VaList)` and
/// `VaList<'_>` built from a variadic `extern "C" fn(…, mut args: ...)`
/// body). Both expose the same `.arg::<T>()` API but as of current
/// nightly Rust are the same concrete type — so implementing this trait
/// for `VaList` covers both entry points.
///
/// The trait exists so the format walker in [`format_into`] can accept
/// either source without duplicating the logic. It's also what makes
/// this module unit-testable natively: tests implement `VaArgSource` on
/// a tiny `Vec`-backed mock.
pub trait VaArgSource {
    /// Read the next `i32` argument.
    ///
    /// # Safety
    /// Caller must ensure the next variadic argument is ABI-compatible
    /// with `i32`.
    unsafe fn next_i32(&mut self) -> i32;
    /// Read the next `u32` argument. Safety as `next_i32`.
    unsafe fn next_u32(&mut self) -> u32;
    /// Read the next `i64` argument. Safety as `next_i32`.
    unsafe fn next_i64(&mut self) -> i64;
    /// Read the next `u64` argument. Safety as `next_i32`.
    unsafe fn next_u64(&mut self) -> u64;
    /// Read the next `isize` argument. Safety as `next_i32`.
    unsafe fn next_isize(&mut self) -> isize;
    /// Read the next `usize` argument. Safety as `next_i32`.
    unsafe fn next_usize(&mut self) -> usize;
    /// Read the next `f64` argument. Safety as `next_i32`.
    unsafe fn next_f64(&mut self) -> f64;
    /// Read the next `*const c_char` argument. Safety as `next_i32`.
    unsafe fn next_cstr(&mut self) -> *const c_char;
    /// Read the next `*const c_void` argument. Safety as `next_i32`.
    unsafe fn next_voidp(&mut self) -> *const c_void;
}

/// Format a single `%`-directive given its parsed spec, conversion
/// character, and argument source. Appends the width-padded result to
/// `out` respecting the remaining buffer budget.
///
/// # Safety
/// Caller must ensure `args` has at least one argument available of
/// the type implied by `(spec.length, conv)`.
pub unsafe fn render_one<V: VaArgSource + ?Sized>(
    out: &mut Vec<u8>,
    size: usize,
    spec: &FormatSpec,
    conv: u8,
    args: &mut V,
) {
    // Safety: this whole fn is unsafe; every `args.next_*` + ptr-read
    // below inherits the fn's contract. Rust 2024 requires an explicit
    // `unsafe {}` block inside `unsafe fn` — we take one wrapping the
    // full body.
    unsafe {
        let mut rendered: Vec<u8> = Vec::with_capacity(32);
        match conv {
            b'd' | b'i' => {
                let v: i64 = match spec.length {
                    LengthMod::None | LengthMod::L => args.next_i32() as i64,
                    LengthMod::LL | LengthMod::BigL => args.next_i64(),
                    LengthMod::Z => args.next_isize() as i64,
                };
                render_signed(&mut rendered, v, spec);
            }
            b'u' => {
                let v: u128 = match spec.length {
                    LengthMod::None | LengthMod::L => args.next_u32() as u128,
                    LengthMod::LL | LengthMod::BigL => args.next_u64() as u128,
                    LengthMod::Z => args.next_usize() as u128,
                };
                render_unsigned(&mut rendered, v, spec);
            }
            b'x' | b'X' => {
                let v: u128 = match spec.length {
                    LengthMod::None | LengthMod::L => args.next_u32() as u128,
                    LengthMod::LL | LengthMod::BigL => args.next_u64() as u128,
                    LengthMod::Z => args.next_usize() as u128,
                };
                render_hex(&mut rendered, v, spec, conv == b'X');
            }
            b'p' => {
                let v = args.next_voidp();
                render_ptr(&mut rendered, v as usize);
            }
            b's' => {
                let ptr = args.next_cstr();
                if ptr.is_null() {
                    rendered.extend_from_slice(b"(null)");
                } else {
                    // Walk to the null terminator manually so the
                    // caller can pass a pointer into arbitrary memory
                    // (C `const char *`).
                    let mut len = 0usize;
                    while *ptr.add(len) != 0 {
                        len += 1;
                    }
                    let bytes = std::slice::from_raw_parts(ptr as *const u8, len);
                    let slice = match spec.precision {
                        Some(p) if p < bytes.len() => &bytes[..p],
                        _ => bytes,
                    };
                    rendered.extend_from_slice(slice);
                }
            }
            b'c' => {
                let v = args.next_i32();
                rendered.push(v as u8);
            }
            b'g' | b'G' => {
                let v = args.next_f64();
                render_g(&mut rendered, v, spec);
            }
            b'%' => {
                rendered.push(b'%');
            }
            _ => {
                rendered.push(b'%');
                rendered.push(conv);
            }
        }
        write_padded(out, size, &rendered, spec);
    }
}

/// Walk the format string, consuming args from `args`, writing output
/// to `out` with buffer-size accounting. `size` includes the space
/// reserved for the null terminator; this function writes at most
/// `size - 1` bytes.
///
/// # Safety
/// Caller must ensure `args` has arguments of the types implied by each
/// `%`-directive in `format`.
pub unsafe fn format_into<V: VaArgSource + ?Sized>(
    out: &mut Vec<u8>,
    size: usize,
    format: &[u8],
    args: &mut V,
) {
    // Safety: inherits this fn's contract; calls into `render_one`
    // which also inherits it.
    unsafe {
        let mut i = 0;
        while i < format.len() {
            let b = format[i];
            if b == b'%' {
                if i + 1 >= format.len() {
                    break;
                }
                i += 1;
                let (spec, conv, j) = parse_spec(format, i);
                if conv == 0 {
                    break;
                }
                render_one(out, size, &spec, conv, args);
                i = j + 1;
            } else {
                if out.len() < size.saturating_sub(1) {
                    out.push(b);
                }
                i += 1;
            }
        }
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// `Vec`-backed mock arg source that drives the formatter from
    /// typed pushes, so we can test format rendering without a real
    /// variadic frame.
    #[derive(Default)]
    struct MockArgs {
        ints: VecDeque<i64>,
        uints: VecDeque<u64>,
        floats: VecDeque<f64>,
        ptrs: VecDeque<usize>,
        cstrs: VecDeque<Vec<u8>>,
    }

    impl VaArgSource for MockArgs {
        unsafe fn next_i32(&mut self) -> i32 {
            self.ints.pop_front().expect("no i32") as i32
        }
        unsafe fn next_u32(&mut self) -> u32 {
            self.uints.pop_front().expect("no u32") as u32
        }
        unsafe fn next_i64(&mut self) -> i64 {
            self.ints.pop_front().expect("no i64")
        }
        unsafe fn next_u64(&mut self) -> u64 {
            self.uints.pop_front().expect("no u64")
        }
        unsafe fn next_isize(&mut self) -> isize {
            self.ints.pop_front().expect("no isize") as isize
        }
        unsafe fn next_usize(&mut self) -> usize {
            self.uints.pop_front().expect("no usize") as usize
        }
        unsafe fn next_f64(&mut self) -> f64 {
            self.floats.pop_front().expect("no f64")
        }
        unsafe fn next_voidp(&mut self) -> *const c_void {
            self.ptrs.pop_front().expect("no ptr") as *const c_void
        }
        unsafe fn next_cstr(&mut self) -> *const c_char {
            let mut owned = self.cstrs.pop_front().expect("no cstr");
            owned.push(0);
            // Leak the buffer for the lifetime of the test run — the
            // alternative is a self-referential struct, not worth it.
            let boxed = owned.into_boxed_slice();
            Box::leak(boxed).as_ptr() as *const c_char
        }
    }

    fn render(fmt: &str, args: &mut MockArgs, size: usize) -> String {
        let mut out: Vec<u8> = Vec::new();
        unsafe { format_into(&mut out, size, fmt.as_bytes(), args) };
        String::from_utf8(out).expect("valid UTF-8")
    }

    #[test]
    fn literal_passthrough() {
        let mut args = MockArgs::default();
        assert_eq!(render("hello", &mut args, 100), "hello");
    }

    #[test]
    fn spec_d() {
        let mut args = MockArgs::default();
        args.ints.push_back(42);
        assert_eq!(render("%d", &mut args, 100), "42");
    }

    #[test]
    fn spec_d_negative() {
        let mut args = MockArgs::default();
        args.ints.push_back(-7);
        assert_eq!(render("%d", &mut args, 100), "-7");
    }

    #[test]
    fn spec_u() {
        let mut args = MockArgs::default();
        args.uints.push_back(42);
        assert_eq!(render("%u", &mut args, 100), "42");
    }

    #[test]
    fn spec_lld() {
        let mut args = MockArgs::default();
        args.ints.push_back(1_234_567_890_123);
        assert_eq!(render("%lld", &mut args, 100), "1234567890123");
    }

    #[test]
    fn spec_zu() {
        let mut args = MockArgs::default();
        args.uints.push_back(65536);
        assert_eq!(render("%zu", &mut args, 100), "65536");
    }

    #[test]
    fn spec_x() {
        let mut args = MockArgs::default();
        args.uints.push_back(0xdeadbeef);
        assert_eq!(render("%x", &mut args, 100), "deadbeef");
    }

    #[test]
    fn spec_x_upper() {
        let mut args = MockArgs::default();
        args.uints.push_back(0xdeadbeef);
        assert_eq!(render("%X", &mut args, 100), "DEADBEEF");
    }

    #[test]
    fn spec_x_alternate() {
        let mut args = MockArgs::default();
        args.uints.push_back(0x2a);
        assert_eq!(render("%#x", &mut args, 100), "0x2a");
    }

    #[test]
    fn spec_p() {
        let mut args = MockArgs::default();
        args.ptrs.push_back(0x1234);
        assert_eq!(render("%p", &mut args, 100), "0x1234");
    }

    #[test]
    fn spec_p_null() {
        let mut args = MockArgs::default();
        args.ptrs.push_back(0);
        assert_eq!(render("%p", &mut args, 100), "0x0");
    }

    #[test]
    fn spec_s() {
        let mut args = MockArgs::default();
        args.cstrs.push_back(b"hello".to_vec());
        assert_eq!(render("%s", &mut args, 100), "hello");
    }

    #[test]
    fn spec_s_precision_truncates() {
        let mut args = MockArgs::default();
        args.cstrs.push_back(b"abcdef".to_vec());
        assert_eq!(render("%.3s", &mut args, 100), "abc");
    }

    #[test]
    fn spec_c() {
        let mut args = MockArgs::default();
        args.ints.push_back(b'A' as i64);
        assert_eq!(render("%c", &mut args, 100), "A");
    }

    #[test]
    fn spec_percent_literal() {
        let mut args = MockArgs::default();
        assert_eq!(render("100%%", &mut args, 100), "100%");
    }

    #[test]
    fn width_right_justify() {
        let mut args = MockArgs::default();
        args.ints.push_back(42);
        assert_eq!(render("%5d", &mut args, 100), "   42");
    }

    #[test]
    fn width_left_justify() {
        let mut args = MockArgs::default();
        args.ints.push_back(42);
        assert_eq!(render("%-5d|", &mut args, 100), "42   |");
    }

    #[test]
    fn width_zero_pad() {
        let mut args = MockArgs::default();
        args.ints.push_back(42);
        assert_eq!(render("%05d", &mut args, 100), "00042");
    }

    #[test]
    fn sign_flag_plus() {
        let mut args = MockArgs::default();
        args.ints.push_back(7);
        assert_eq!(render("%+d", &mut args, 100), "+7");
    }

    #[test]
    fn sign_flag_space() {
        let mut args = MockArgs::default();
        args.ints.push_back(7);
        assert_eq!(render("% d", &mut args, 100), " 7");
    }

    #[test]
    fn precision_min_digits_int() {
        let mut args = MockArgs::default();
        args.ints.push_back(7);
        assert_eq!(render("%.3d", &mut args, 100), "007");
    }

    #[test]
    fn g_simple() {
        let mut args = MockArgs::default();
        args.floats.push_back(3.14);
        assert_eq!(render("%g", &mut args, 100), "3.14");
    }

    #[test]
    fn g_integer_value() {
        let mut args = MockArgs::default();
        args.floats.push_back(100.0);
        assert_eq!(render("%g", &mut args, 100), "100");
    }

    #[test]
    fn g_tiny_uses_exp() {
        let mut args = MockArgs::default();
        args.floats.push_back(0.000_001);
        let out = render("%g", &mut args, 100);
        assert!(
            out.contains('e'),
            "expected exponent for 1e-6, got {:?}",
            out
        );
    }

    #[test]
    fn g_large_uses_exp() {
        let mut args = MockArgs::default();
        args.floats.push_back(1e20);
        let out = render("%g", &mut args, 100);
        assert!(
            out.contains('e'),
            "expected exponent for 1e20, got {:?}",
            out
        );
    }

    #[test]
    fn g_nan() {
        let mut args = MockArgs::default();
        args.floats.push_back(f64::NAN);
        assert_eq!(render("%g", &mut args, 100), "nan");
    }

    #[test]
    fn g_neg_inf() {
        let mut args = MockArgs::default();
        args.floats.push_back(f64::NEG_INFINITY);
        assert_eq!(render("%g", &mut args, 100), "-inf");
    }

    #[test]
    fn buffer_truncation_null_terminates() {
        let mut args = MockArgs::default();
        args.cstrs.push_back(b"abcdef".to_vec());
        // With `size = 4`, we can write 3 chars + null.
        let mut out: Vec<u8> = Vec::new();
        unsafe { format_into(&mut out, 4, b"%s", &mut args) };
        assert_eq!(out, vec![b'a', b'b', b'c']);
    }

    #[test]
    fn mixed_format() {
        let mut args = MockArgs::default();
        args.cstrs.push_back(b"world".to_vec());
        args.ints.push_back(42);
        assert_eq!(
            render("hello %s, answer = %d", &mut args, 100),
            "hello world, answer = 42"
        );
    }
}
