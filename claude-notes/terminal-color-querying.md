# Terminal Color Querying

## Overview

Modern terminals support querying their current foreground and background colors using ANSI escape sequences (OSC 10/11). This could be used to implement smarter color selection in the ANSI writer.

## Query Sequences

### Foreground Color
```
\x1b]10;?\x1b\\
```
or with BEL terminator:
```
\x1b]10;?\x07
```

### Background Color
```
\x1b]11;?\x1b\\
```
or with BEL terminator:
```
\x1b]11;?\x07
```

## Response Format

The terminal responds by writing to stdout:
```
\x1b]10;rgb:RRRR/GGGG/BBBB\x1b\\
```
or
```
\x1b]11;rgb:RRRR/GGGG/BBBB\x1b\\
```

Where `RRRR`, `GGGG`, `BBBB` are 4-digit hexadecimal values.

**Example responses:**
- White: `rgb:ffff/ffff/ffff`
- Black: `rgb:0000/0000/0000`
- Gray: `rgb:8080/8080/8080`

## Parsing

The response can be parsed with a regex like:
```
\x1b](?:10|11);(.+)(?:\x1b\\|\x07)
```

Capturing group 1 contains the color string (e.g., `rgb:ffff/ffff/ffff`).

## Terminal Compatibility

**Supported by:**
- xterm
- iTerm2
- GNOME Terminal (libvte-based)
- Konsole
- Windows Terminal
- Most modern terminal emulators

**Not reliable in:**
- Pipes and redirects
- Non-interactive contexts
- Some minimal terminal implementations

## Implementation Requirements

To query terminal colors:

1. **Terminal must be in raw/cbreak mode** to read the response
2. **Write query to stdout**
3. **Read response from stdin**
4. **Parse RGB values** from response
5. **Restore terminal mode** after query

## Use Cases for ANSI Writer

### Smart Color Selection
- Choose contrasting colors based on actual background
- Avoid white text on white backgrounds
- Avoid black text on black backgrounds

### Dark/Light Mode Detection
Calculate luminance from background RGB:
```
L = 0.299*R + 0.587*G + 0.114*B
```
- If L > 0.5: light background (use darker colors)
- If L < 0.5: dark background (use brighter colors)

### Better Header Styling
- H1 on dark background: use bright white
- H1 on light background: use dark/black
- Automatically adjust muted colors

### Adaptive Syntax Highlighting
- Adjust code block colors based on background
- Ensure sufficient contrast ratios

## Challenges

1. **Availability**: Not always available (pipes, files, non-terminals)
2. **Performance**: Adds latency (terminal roundtrip)
3. **Complexity**: Requires terminal mode manipulation
4. **Error Handling**: Need fallbacks when query fails
5. **Caching**: Should cache results to avoid repeated queries

## Current Approach

Our current color choices work reasonably well on both light and dark terminals:
- `Color::White` (bright) - readable on dark, acceptable on light
- `Color::DarkGrey` (muted) - readable on both
- `Color::Cyan` (links) - good contrast on both

## Recommendation

**Low priority enhancement** - implement as an optional feature:
1. Add `detect_terminal_colors` method to `AnsiConfig`
2. Use detected colors to adjust palette
3. Fall back to current defaults if detection fails
4. Gate behind feature flag (like `terminal-hyperlinks`)

## References

- [Knowledge Bits — Getting a Terminal's Default Foreground & Background Colors](https://jwodder.github.io/kbits/posts/term-fgbg/)
- [ANSI Escape Codes Gist](https://gist.github.com/fnky/458719343aabd01cfb17a3a4f7296797)
- OSC (Operating System Command) sequences specification
