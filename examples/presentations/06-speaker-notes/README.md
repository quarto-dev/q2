# 06-speaker-notes — Speaker notes

Attaching presenter-only notes to a slide with the `.notes` class.

## What this demonstrates

- **Speaker notes.** A `.notes` div holds commentary for the presenter that
  does not appear on the slide.

## How to run

```bash
cargo run --bin q2 -- render examples/presentations/06-speaker-notes
```

## What to look for

- The note renders as `<aside class="notes">` and is hidden on the slide.
- Showing notes in a live speaker view (the reveal.js notes plugin, the `S`
  key) is not wired up yet — the markup is produced, the presenter UI is
  future work.
