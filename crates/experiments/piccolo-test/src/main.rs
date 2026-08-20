use lua_patterns::LuaPattern;
use piccolo::{Callback, CallbackReturn, Closure, Executor, Lua, StashedExecutor, Table, Value};

fn main() {
    println!("=== Phase 1.3: Minimal filter test (all-in-Lua) ===");
    test_uppercase_filter_lua();

    println!("\n=== Phase 1.3b: Rust-side table interop ===");
    test_rust_table_interop();

    println!("\n=== Phase 1.4: String metatable test ===");
    test_string_metatable();

    println!("\n=== Phase 1.4b: Method syntax on table field ===");
    test_method_syntax_in_filter();

    println!("\n=== Phase 1.5: Rust callback registration ===");
    test_rust_callback();

    println!("\n=== Phase 1.5b: lua-patterns integration ===");
    test_lua_patterns_integration();

    println!("\n=== Phase 1.5c: Pattern methods via string metatable ===");
    test_pattern_methods();

    println!("\n=== All tests complete ===");
}

fn load_and_run(lua: &mut Lua, name: &str, script: &[u8]) -> StashedExecutor {
    lua.try_enter(|ctx| {
        let closure = Closure::load(ctx, Some(name), script).unwrap();
        Ok(ctx.stash(Executor::start(ctx, closure.into(), ())))
    })
    .unwrap()
}

/// Register string.find, string.match, string.gsub using lua-patterns
fn register_string_patterns(lua: &mut Lua) {
    lua.enter(|ctx| {
        let string_table: Table = ctx.get_global("string").unwrap();

        // string.find(s, pattern [, init [, plain]])
        string_table.set_field(
            ctx,
            "find",
            Callback::from_fn(&ctx, |ctx, _, mut stack| {
                let (s, pattern, init, plain): (
                    piccolo::String,
                    piccolo::String,
                    Option<i64>,
                    Option<bool>,
                ) = stack.consume(ctx)?;

                let s_bytes = s.as_bytes();
                let pat_bytes = pattern.as_bytes();
                let plain = plain.unwrap_or(false);
                let init = init.unwrap_or(1);
                let start_idx = if init > 0 {
                    (init - 1) as usize
                } else if init < 0 {
                    s_bytes.len().saturating_sub(init.unsigned_abs() as usize)
                } else {
                    0
                };

                if start_idx >= s_bytes.len() {
                    stack.replace(ctx, Value::Nil);
                    return Ok(CallbackReturn::Return);
                }

                let search_slice = &s_bytes[start_idx..];

                if plain {
                    // Plain string search (no patterns)
                    if let Some(pos) = search_slice
                        .windows(pat_bytes.len())
                        .position(|w| w == pat_bytes)
                    {
                        stack.push_back(Value::Integer((start_idx + pos + 1) as i64));
                        stack.push_back(Value::Integer((start_idx + pos + pat_bytes.len()) as i64));
                    } else {
                        stack.replace(ctx, Value::Nil);
                    }
                } else {
                    let pat_str = std::str::from_utf8(pat_bytes).unwrap_or("");
                    let search_str = std::str::from_utf8(search_slice).unwrap_or("");
                    match LuaPattern::new_try(pat_str) {
                        Ok(mut m) => {
                            if m.matches(search_str) {
                                let range = m.range();
                                stack.push_back(Value::Integer(
                                    (start_idx + range.start + 1) as i64,
                                ));
                                stack.push_back(Value::Integer((start_idx + range.end) as i64));
                                // Push captures if any (beyond capture 0)
                                if m.capture(0) != m.range() || m.captures(search_str).len() > 1 {
                                    let caps = m.captures(search_str);
                                    for cap in caps.iter().skip(1) {
                                        stack.push_back(Value::String(ctx.intern(cap.as_bytes())));
                                    }
                                }
                            } else {
                                stack.replace(ctx, Value::Nil);
                            }
                        }
                        Err(_) => {
                            stack.replace(ctx, Value::Nil);
                        }
                    }
                }
                Ok(CallbackReturn::Return)
            }),
        );

        // string.match(s, pattern [, init])
        string_table.set_field(
            ctx,
            "match",
            Callback::from_fn(&ctx, |ctx, _, mut stack| {
                let (s, pattern, init): (piccolo::String, piccolo::String, Option<i64>) =
                    stack.consume(ctx)?;

                let s_str = std::str::from_utf8(s.as_bytes()).unwrap_or("");
                let pat_str = std::str::from_utf8(pattern.as_bytes()).unwrap_or("");
                let init = init.unwrap_or(1);
                let start_idx = if init > 0 {
                    (init - 1) as usize
                } else {
                    s_str.len().saturating_sub(init.unsigned_abs() as usize)
                };

                let search_str = if start_idx < s_str.len() {
                    &s_str[start_idx..]
                } else {
                    ""
                };

                match LuaPattern::new_try(pat_str) {
                    Ok(mut m) => {
                        if m.matches(search_str) {
                            let caps = m.captures(search_str);
                            if caps.len() > 1 {
                                // Return captures (skip full match)
                                for cap in caps.iter().skip(1) {
                                    stack.push_back(Value::String(ctx.intern(cap.as_bytes())));
                                }
                            } else {
                                // No explicit captures: return whole match
                                stack.push_back(Value::String(ctx.intern(caps[0].as_bytes())));
                            }
                        } else {
                            stack.replace(ctx, Value::Nil);
                        }
                    }
                    Err(_) => {
                        stack.replace(ctx, Value::Nil);
                    }
                }
                Ok(CallbackReturn::Return)
            }),
        );

        // string.gsub(s, pattern, repl [, n])
        string_table.set_field(
            ctx,
            "gsub",
            Callback::from_fn(&ctx, |ctx, _, mut stack| {
                let (s, pattern, repl, _n): (
                    piccolo::String,
                    piccolo::String,
                    piccolo::String,
                    Option<i64>,
                ) = stack.consume(ctx)?;

                let s_str = std::str::from_utf8(s.as_bytes()).unwrap_or("");
                let pat_str = std::str::from_utf8(pattern.as_bytes()).unwrap_or("");
                let repl_str = std::str::from_utf8(repl.as_bytes()).unwrap_or("");

                match LuaPattern::new_try(pat_str) {
                    Ok(mut m) => {
                        let result = m.gsub(s_str, repl_str);
                        stack.push_back(Value::String(ctx.intern(result.as_bytes())));
                    }
                    Err(_) => {
                        stack.push_back(Value::String(s));
                    }
                }
                Ok(CallbackReturn::Return)
            }),
        );

        // string.rep(s, n [, sep])
        string_table.set_field(
            ctx,
            "rep",
            Callback::from_fn(&ctx, |ctx, _, mut stack| {
                let (s, n, sep): (piccolo::String, i64, Option<piccolo::String>) =
                    stack.consume(ctx)?;

                let s_str = std::str::from_utf8(s.as_bytes()).unwrap_or("");
                let sep_str = sep
                    .as_ref()
                    .map(|s| std::str::from_utf8(s.as_bytes()).unwrap_or(""))
                    .unwrap_or("");

                let n = n.max(0) as usize;
                let mut result = std::string::String::new();
                for i in 0..n {
                    if i > 0 {
                        result.push_str(sep_str);
                    }
                    result.push_str(s_str);
                }
                stack.replace(ctx, ctx.intern(result.as_bytes()));
                Ok(CallbackReturn::Return)
            }),
        );
    });
}

// ---- Tests ----

fn test_uppercase_filter_lua() {
    let mut lua = Lua::full();
    let script = br#"
function Str(elem)
    elem.c = string.upper(elem.c)
    return elem
end
local t = {t="Str", c="hello"}
return Str(t).c
"#;
    let ex = load_and_run(&mut lua, "filter.lua", script);
    let result: String = lua.execute::<String>(&ex).unwrap();
    assert_eq!(result, "HELLO");
    println!("SUCCESS: Str filter returned c=\"{}\"", result);
}

fn test_rust_table_interop() {
    let mut lua = Lua::full();
    let script = br#"
function Str(elem)
    elem.c = string.upper(elem.c)
    return elem
end
"#;
    let ex = load_and_run(&mut lua, "filter.lua", script);
    lua.execute::<()>(&ex).unwrap();

    let call_ex: StashedExecutor = lua
        .try_enter(|ctx| {
            let table = Table::new(&ctx);
            table.set(ctx, "t", "Str").unwrap();
            table.set(ctx, "c", "hello").unwrap();
            let str_fn: piccolo::Closure = ctx.get_global("Str").unwrap();
            let exec = Executor::start(ctx, str_fn.into(), (table,));
            Ok(ctx.stash(exec))
        })
        .unwrap();

    lua.finish(&call_ex).unwrap();

    let c_value: String = lua
        .try_enter(|ctx| {
            let executor = ctx.fetch(&call_ex);
            let result: Value = executor.take_result(ctx).unwrap().unwrap();
            match result {
                Value::Table(t) => {
                    let c: piccolo::String = t.get(ctx, "c")?;
                    Ok(std::str::from_utf8(c.as_bytes()).unwrap().to_string())
                }
                other => panic!("Expected table, got {:?}", other),
            }
        })
        .unwrap();

    assert_eq!(c_value, "HELLO");
    println!("SUCCESS: Rust→Lua table interop works, c=\"{}\"", c_value);
}

fn test_string_metatable() {
    let mut lua = Lua::full();
    let ex = load_and_run(&mut lua, "test.lua", br#"return ("hello"):upper()"#);
    let result: String = lua.execute::<String>(&ex).unwrap();
    assert_eq!(result, "HELLO");
    println!("SUCCESS: (\"hello\"):upper() = \"{}\"", result);
}

fn test_method_syntax_in_filter() {
    let mut lua = Lua::full();
    let script = br#"
function Str(elem)
    elem.c = elem.c:upper()
    return elem
end
local t = {t="Str", c="hello"}
return Str(t).c
"#;
    let ex = load_and_run(&mut lua, "filter.lua", script);
    let result: String = lua.execute::<String>(&ex).unwrap();
    assert_eq!(result, "HELLO");
    println!("SUCCESS: elem.c:upper() = \"{}\"", result);
}

fn test_rust_callback() {
    let mut lua = Lua::full();
    lua.enter(|ctx| {
        let callback = Callback::from_fn(&ctx, |ctx, _exec, mut stack| {
            let s: piccolo::String = stack.consume(ctx)?;
            let reversed: Vec<u8> = s.as_bytes().iter().copied().rev().collect();
            let result = ctx.intern(&reversed);
            stack.replace(ctx, result);
            Ok(CallbackReturn::Return)
        });
        ctx.set_global("my_reverse", callback);
    });

    let ex = load_and_run(&mut lua, "test.lua", br#"return my_reverse("hello world")"#);
    let result: String = lua.execute::<String>(&ex).unwrap();
    assert_eq!(result, "dlrow olleh");
    println!(
        "SUCCESS: Rust callback my_reverse(\"hello world\") = \"{}\"",
        result
    );
}

fn test_lua_patterns_integration() {
    let mut lua = Lua::full();
    register_string_patterns(&mut lua);

    // Test string.find
    let ex = load_and_run(
        &mut lua,
        "test.lua",
        br#"
local s, e = string.find("hello world", "world")
return s .. "," .. e
"#,
    );
    let result: String = lua.execute::<String>(&ex).unwrap();
    assert_eq!(result, "7,11");
    println!(
        "SUCCESS: string.find(\"hello world\", \"world\") = {}",
        result
    );

    // Test string.find with pattern
    let ex = load_and_run(
        &mut lua,
        "test.lua",
        br#"
local s, e, cap = string.find("hello 42 world", "(%d+)")
return s .. "," .. e .. "," .. cap
"#,
    );
    let result: String = lua.execute::<String>(&ex).unwrap();
    assert_eq!(result, "7,8,42");
    println!("SUCCESS: string.find with capture pattern = {}", result);

    // Test string.match
    let ex = load_and_run(
        &mut lua,
        "test.lua",
        br#"return string.match("hello 42 world", "(%d+)")"#,
    );
    let result: String = lua.execute::<String>(&ex).unwrap();
    assert_eq!(result, "42");
    println!(
        "SUCCESS: string.match(\"hello 42 world\", \"(%%d+)\") = {}",
        result
    );

    // Test string.gsub
    let ex = load_and_run(
        &mut lua,
        "test.lua",
        br#"return string.gsub("hello world", "world", "lua")"#,
    );
    let result: String = lua.execute::<String>(&ex).unwrap();
    assert_eq!(result, "hello lua");
    println!(
        "SUCCESS: string.gsub(\"hello world\", \"world\", \"lua\") = {}",
        result
    );

    // Test string.rep
    let ex = load_and_run(&mut lua, "test.lua", br#"return string.rep("ab", 3, ",")"#);
    let result: String = lua.execute::<String>(&ex).unwrap();
    assert_eq!(result, "ab,ab,ab");
    println!("SUCCESS: string.rep(\"ab\", 3, \",\") = {}", result);
}

fn test_pattern_methods() {
    let mut lua = Lua::full();
    register_string_patterns(&mut lua);

    // Test method syntax: s:find(), s:match(), s:gsub()
    let ex = load_and_run(
        &mut lua,
        "test.lua",
        br#"return ("hello world"):gsub("world", "piccolo")"#,
    );
    let result: String = lua.execute::<String>(&ex).unwrap();
    assert_eq!(result, "hello piccolo");
    println!(
        "SUCCESS: (\"hello world\"):gsub(\"world\", \"piccolo\") = {}",
        result
    );

    // Test a more realistic filter using pattern matching
    let ex = load_and_run(
        &mut lua,
        "test.lua",
        br#"
function Str(elem)
    -- Replace "foo" with "bar" in all Str elements
    elem.c = elem.c:gsub("foo", "bar")
    return elem
end

local t = {t="Str", c="I like foo and foo"}
local result = Str(t)
return result.c
"#,
    );
    let result: String = lua.execute::<String>(&ex).unwrap();
    assert_eq!(result, "I like bar and bar");
    println!("SUCCESS: filter with gsub: \"{}\"", result);

    // Test s:match()
    let ex = load_and_run(
        &mut lua,
        "test.lua",
        br#"return ("hello 42"):match("(%d+)")"#,
    );
    let result: String = lua.execute::<String>(&ex).unwrap();
    assert_eq!(result, "42");
    println!("SUCCESS: (\"hello 42\"):match(\"(%%d+)\") = {}", result);

    // Test s:rep()
    let ex = load_and_run(&mut lua, "test.lua", br#"return ("ha"):rep(3)"#);
    let result: String = lua.execute::<String>(&ex).unwrap();
    assert_eq!(result, "hahaha");
    println!("SUCCESS: (\"ha\"):rep(3) = {}", result);
}
