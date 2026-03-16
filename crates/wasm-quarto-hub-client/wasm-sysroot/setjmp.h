#pragma once

/*
 * Stub setjmp.h for wasm32-unknown-unknown.
 *
 * Lua's ldo.c includes <setjmp.h>, but our luaconf_wasm.h overrides
 * LUAI_TRY/LUAI_THROW/luai_jmpbuf so setjmp/longjmp are never actually
 * used. We just need the header to exist so the #include doesn't fail.
 */

typedef int jmp_buf[1];

int setjmp(jmp_buf env);
_Noreturn void longjmp(jmp_buf env, int val);
