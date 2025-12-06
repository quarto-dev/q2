## Fuzzing

You'll need rust nightly, and you'll need to run `cargo fuzz` slightly differently because this is a package inside a workspace. Do this:

```
$ cargo fuzz run hello_fuzz --fuzz-dir ./fuzz
```
