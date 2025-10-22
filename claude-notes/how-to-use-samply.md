# How to Use samply for Profiling Rust Projects

**samply** is a modern, user-friendly profiling tool that works great on macOS and produces interactive visualizations in your browser.

## Installation

```bash
cargo install samply
```

That's it! No special permissions or system configuration needed.

## Basic Usage

### 1. Profile a Command (Interactive Mode)

The simplest way - runs your program and opens an interactive flamegraph in your browser:

```bash
samply record cargo run --release --bin my-binary -- arg1 arg2
```

**What happens:**
1. samply starts profiling
2. Your program runs
3. When it finishes, samply automatically opens Firefox Profiler in your browser
4. You can explore the flamegraph interactively

**Example from our project:**
```bash
samply record cargo run --release --bin qmd-syntax-helper -- \
  check --rule div-whitespace 'external-sites/**/*.qmd'
```

### 2. Save Profile for Later Analysis

If you want to save the profile data without opening the browser immediately:

```bash
samply record --save-only --output profile.json \
  cargo run --release --bin my-binary -- args
```

Then later, view it:
```bash
samply load profile.json
```

This opens the interactive viewer in your browser.

## Understanding the Output

### Interactive Viewer (Firefox Profiler)

When samply opens the browser, you'll see several views:

#### 1. **Call Tree View** (Default)
- Shows functions in a tree structure
- Sorted by "self time" (time spent IN that function)
- Click to expand and see what it calls

**How to read it:**
- **Self time**: Time spent in the function itself (not including callees)
- **Total time**: Time including all functions it calls
- Look for high self time to find actual bottlenecks

#### 2. **Flame Graph**
- Click the "Flame Graph" tab at the top
- Visual representation of call stacks
- Width = time spent
- Height = call depth

**How to read it:**
- Wide bars = expensive operations
- Tall stacks = deep call chains
- Click any bar to zoom in
- Right-click → "Copy function name" is useful

#### 3. **Stack Chart**
- Timeline view showing what's running when
- Good for seeing if work is evenly distributed

### Key Navigation Tips

- **Search (Ctrl+F)**: Find functions by name
- **Filter**: Use the search box to filter to specific functions
- **Zoom**: Click and drag to zoom into a time range
- **Compare**: Load multiple profiles to compare

## Common Workflows

### Workflow 1: "Where is my program slow?"

```bash
# 1. Profile your program
samply record cargo run --release --bin my-app -- typical-workload

# 2. In the browser:
#    - Look at "Call Tree" view
#    - Sort by "Self Time" (should be default)
#    - The top functions are where time is spent

# 3. Click "Flame Graph" tab
#    - Find the widest bars
#    - These are your bottlenecks
```

### Workflow 2: "Did my optimization work?"

```bash
# Before optimization
samply record --save-only --output before.json \
  cargo run --release --bin my-app -- workload

# Make your changes...

# After optimization
samply record --save-only --output after.json \
  cargo run --release --bin my-app -- workload

# Compare (open both in browser tabs side-by-side)
samply load before.json  # Opens in browser
samply load after.json   # Opens in another tab

# Look for:
# - Total time reduction
# - Specific function time reduction
# - Functions that disappeared from top of call tree
```

### Workflow 3: "Profile just one function"

```bash
# 1. Profile normally
samply record cargo run --release --bin my-app -- args

# 2. In browser, use the search box
#    Type the function name: "my_expensive_function"

# 3. Click "Filter to this call stack"
#    Now you only see time spent in that function and its callees
```

## Tips for Better Profiling

### 1. Always Use Release Mode

```bash
# ❌ Don't do this (debug builds are 10-100x slower)
samply record cargo run --bin my-app -- args

# ✅ Do this
samply record cargo run --release --bin my-app -- args
```

Debug builds have no optimizations, so profiling them is misleading.

### 2. Enable Debug Symbols in Release

For better function names in the profiler, add to your `Cargo.toml`:

```toml
[profile.release]
debug = true  # Adds symbols without disabling optimizations
```

Or set environment variable:
```bash
CARGO_PROFILE_RELEASE_DEBUG=true samply record cargo run --release ...
```

### 3. Profile Realistic Workloads

```bash
# ❌ Toy example
samply record cargo run --release -- process file.txt

# ✅ Real workload
samply record cargo run --release -- process 'real-data/**/*.txt'
```

Small inputs might not reveal the real bottlenecks.

### 4. Multiple Runs for Consistency

```bash
# Run 3 times, compare
for i in 1 2 3; do
  samply record --save-only --output run-$i.json \
    cargo run --release --bin my-app -- args
done

# Look for consistency across runs
samply load run-1.json
samply load run-2.json
samply load run-3.json
```

If the profiles look very different, your program might have:
- Non-deterministic behavior
- External dependencies (network, file I/O)
- Initialization costs that dominate short runs

## Reading Flamegraphs

### Anatomy of a Flamegraph

```
┌────────────────────────────────────────────┐
│           main (100%)                      │  ← Bottom = entry point
├────────────┬───────────────────────────────┤
│  parse 60% │       process 40%             │  ← What main calls
├─────┬──────┼─────────┬─────────────────────┤
│ A   │   B  │    C    │         D           │  ← What those call
│ 20% │ 40%  │   20%   │        20%          │
└─────┴──────┴─────────┴─────────────────────┘
     ↑ Top = innermost functions (actual work)
```

**Key insights from this example:**
- `parse` takes 60% of time, `process` takes 40%
- Within `parse`, function `B` takes 40% (the widest bar in parse's children)
- Function `B` is the biggest bottleneck overall

**Look for:**
- **Wide bars** = time spent
- **Tall stacks** = many layers (might indicate inefficiency)
- **Flat/wide bars at top** = actual work (good)
- **Many tiny bars** = lots of small function calls (overhead)

### Common Patterns

#### Pattern 1: Allocation Hell
```
├─────────────────────────────────────┐
│  my_function (50%)                  │
├──────┬──────┬──────┬──────┬─────────┤
│ malloc│ free │memcpy│memset│ actual │
│  15%  │ 15%  │ 10%  │ 10%  │  work  │
└───────┴──────┴──────┴──────┴────────┘
```
**Problem:** More time allocating memory than doing work
**Solution:** Use stack allocation, reduce clones, reuse buffers

#### Pattern 2: Hash Table Overhead
```
├─────────────────────────────────────┐
│  my_function (50%)                  │
├──────────┬──────────┬───────────────┤
│ hash_key │  insert  │  actual_work  │
│   20%    │   20%    │     10%       │
└──────────┴──────────┴───────────────┘
```
**Problem:** HashMap operations dominating (like our TreeSitterLogObserver!)
**Solution:** Consider if HashMap is needed, maybe use arrays/match instead

#### Pattern 3: Deep Recursion
```
│ main │
│  fn_a│
│  fn_a│
│  fn_a│
│  fn_a│  ← Very tall, narrow stack
│  fn_a│
│  fn_a│
│  fn_a│
└─────┘
```
**Problem:** Deep recursion causing overhead
**Solution:** Consider iteration instead, or tail-call optimization

## Advanced Features

### Filtering by Thread

If your program is multi-threaded:

1. In the Firefox Profiler, look for the thread dropdown (top left)
2. Each thread has its own profile
3. Look at each thread to find which one is slow

### Markers and Ranges

samply captures certain events automatically:
- File I/O
- Network operations
- Locks/synchronization

Look for these in the "Marker Chart" view to find external bottlenecks.

### Comparing Before/After

1. Load both profiles in separate tabs
2. Look at the same view in both (e.g., Call Tree)
3. Find the function you optimized
4. Compare self time / total time

**Example:**
```
Before: my_function - Self: 500ms (30%)
After:  my_function - Self: 50ms (5%)
→ 10x improvement! ✅
```

## Troubleshooting

### "No symbols in profile"

**Problem:** Functions show as hex addresses (0x12345) instead of names

**Solution:**
```bash
# Enable debug symbols
CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release

# Then profile
samply record ./target/release/my-binary args
```

### "Profile is empty / no data"

**Problem:** Program runs too fast

**Solution:**
```bash
# Make it run longer
samply record cargo run --release -- process large-input

# Or loop it
samply record sh -c 'for i in {1..100}; do ./my-binary; done'
```

### "Browser doesn't open"

**Problem:** samply can't find your browser

**Solution:**
```bash
# Save profile, open manually
samply record --save-only --output profile.json my-program
samply load profile.json  # Opens browser

# Or visit profiler.firefox.com and drag-drop the JSON file
```

## Real Example: Our Optimization

Here's exactly what I did for the TreeSitterLogObserver optimization:

```bash
# 1. Baseline profile
samply record --save-only --output before.json \
  cargo run --release --bin qmd-syntax-helper -- \
  check --rule div-whitespace 'external-sites/**/*.qmd'

# 2. Made the HashMap → Option optimization

# 3. After profile
samply record --save-only --output after.json \
  cargo run --release --bin qmd-syntax-helper -- \
  check --rule div-whitespace 'external-sites/**/*.qmd'

# 4. Compare
samply load before.json  # Showed HashMap::insert at 8.2%
samply load after.json   # HashMap::insert gone, 2.4x faster overall

# 5. Time both to verify
time cargo run --release ... # Before: 11.66s
time cargo run --release ... # After:  4.85s
```

## Quick Reference Card

```bash
# Install
cargo install samply

# Quick profile (opens browser)
samply record cargo run --release --bin my-app -- args

# Save for later
samply record --save-only --output my-profile.json my-command

# View saved profile
samply load my-profile.json

# Enable debug symbols
CARGO_PROFILE_RELEASE_DEBUG=true samply record cargo run --release ...

# Profile non-Cargo command
samply record ./my-binary arg1 arg2

# Profile shell script
samply record sh -c 'for i in {1..100}; do ./benchmark; done'
```

## Further Resources

- **samply GitHub**: https://github.com/mstange/samply
- **Firefox Profiler docs**: https://profiler.firefox.com/docs/
- **Rust profiling guide**: https://nnethercote.github.io/perf-book/profiling.html

## Summary: When to Use samply

✅ **Use samply when:**
- You want to know "where is my program slow?"
- You want an easy, visual profiling experience
- You're on macOS or Linux
- You want to share profiles with others (just send the JSON)

❌ **Don't use samply when:**
- You need cycle-accurate profiling (use perf on Linux)
- You need hardware counter data (cache misses, branch mispredictions)
- You're profiling memory usage specifically (use heaptrack or dhat)
- You're on Windows (use cargo-flamegraph with dtrace, or Windows Performance Analyzer)

For most Rust development on macOS, **samply is the best choice** for CPU profiling. It's what I used to discover that your HashMap was the bottleneck, and it made the 2.4x speedup obvious!
