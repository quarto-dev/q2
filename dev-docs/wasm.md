## Wasm builds

```
cd crates/wasm-qmd-parser

# To work around this error, because Apple Clang doesn't work with wasm32-unknown-unknown?
# I believe this is not required on a Linux machine.
# Requires `brew install llvm`.
# https://github.com/briansmith/ring/issues/1824
# error: unable to create target: 'No available targets are compatible with triple "wasm32-unknown-unknown"'
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"

# To tell rustc to include our C shims located in `wasm-sysroot`, which we eventually compile into the project
# with `c_shim.rs`.
# https://github.com/tree-sitter/tree-sitter/discussions/1550#discussioncomment-8445285
#
# It also seems like we need to define HAVE_ENDIAN_H to tell tree-sitter we have `endian.h`
# as it doesn't seem to pick up on that automatically?
# https://github.com/tree-sitter/tree-sitter/blob/0be215e152d58351d2691484b4398ceff041f2fb/lib/src/portable/endian.h#L18
export CFLAGS_wasm32_unknown_unknown="-I$(pwd)/wasm-sysroot -Wbad-function-cast -Wcast-function-type -fno-builtin -DHAVE_ENDIAN_H"

# To just build the wasm-qmd-parser crate
# cargo build --target wasm32-unknown-unknown

# To build the wasm-pack bundle
# Note that you'll need `opt-level = "s"` in your `profile.dev` cargo profile
# otherwise you can get a "too many locals" error.
wasm-pack build --target web --dev
```
