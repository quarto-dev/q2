/*
** Wasm32-unknown-unknown overrides for Lua error handling.
**
** This header is force-included (-include) before all Lua source files
** when building for wasm32-unknown-unknown. It overrides Lua's default
** setjmp/longjmp error handling with Rust's panic/catch_unwind mechanism.
**
** The LUAI_TRY macro references local variables `f`, `L`, `ud` by name.
** This is intentionally coupled to luaD_rawrunprotected() in ldo.c — the
** ONLY call site for LUAI_TRY.
*/

#ifndef luaconf_wasm_h
#define luaconf_wasm_h

/* Declare the Rust-provided shim functions */
extern int rust_lua_protected_call(
    void (*f)(void*, void*), void* L, void* ud);
extern _Noreturn void rust_lua_throw(void);

/* Override Lua's error handling macros BEFORE ldo.c defines them */
#define luai_jmpbuf     int  /* dummy, like the C++ path */
#define LUAI_THROW(L,c) rust_lua_throw()
#define LUAI_TRY(L,c,a) \
    if (rust_lua_protected_call((void (*)(void*, void*))f, (void*)L, (void*)ud) != 0) { \
        if ((c)->status == 0) (c)->status = -1; \
    }

#endif
