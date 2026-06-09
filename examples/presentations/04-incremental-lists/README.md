# 04-incremental-lists — Lists revealed item by item

Stepping through list items one click at a time, globally and per-list.

## What this demonstrates

- **Global `incremental: true`.** Every list in the deck steps through its
  items one at a time.
- **`.nonincremental`.** Opts a single list out, so it shows all items at once
  even under the global setting.
- **`.incremental`.** Opts a single list in when the global option is off.

## How to run

```bash
cargo run --bin q2 -- render examples/presentations/04-incremental-lists
```

## What to look for

- Items of an incremental list render as `<li class="fragment">`; reveal.js
  shows them one click at a time.
- The `.nonincremental` list's items are plain `<li>` — all visible at once.
