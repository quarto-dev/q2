# Upload Dialog: Editable Filenames with Whitespace Sanitization

**Beads Issue:** bd-anxz

## Overview

The file upload dialog (`NewFileDialog.tsx`) currently displays uploaded filenames as read-only text. Users cannot change the filename before uploading. Additionally, filenames with whitespace characters (spaces, tabs, non-breaking spaces, etc.) are passed through as-is, which makes them difficult to reference in markdown without escaping. Filenames from macOS screenshots (e.g., `Screenshot 2026-02-13 at 3.54.46 PM.png`) also contain interior dots that cause confusion.

This plan adds:
1. Editable filename fields in the upload dialog's file preview list
2. Automatic sanitization: whitespace → hyphen, interior dots → hyphen

## Current Flow

```
Drag/drop files → NewFileDialog opens → file previews show file.name (read-only)
  → "Upload" button → onUploadBinaryFile(file) → Editor.handleUploadBinaryFile()
  → createBinaryFile(file.name, content, mimeType)
```

Key observation: `file.name` is a read-only property on the browser `File` object, so we can't modify it. Instead, we need to track edited names separately and pass them through the upload chain.

## Design Decisions

### Whitespace sanitization regex

Use `/[\s\u00A0\u2000-\u200B\u2028\u2029\u202F\u205F\u3000\uFEFF]+/g` to catch:
- `\s` — ASCII whitespace (space, tab, newline, carriage return, form feed, vertical tab)
- `\u00A0` — non-breaking space
- `\u2000-\u200B` — en space, em space, thin space, hair space, zero-width space, etc.
- `\u2028` — line separator
- `\u2029` — paragraph separator
- `\u202F` — narrow no-break space
- `\u205F` — medium mathematical space
- `\u3000` — ideographic space (CJK)
- `\uFEFF` — zero-width no-break space (BOM)

Consecutive whitespace characters should collapse into a single `-`.

### Interior dot sanitization

Replace every `.` except the last one (the extension separator) with `-`. This handles macOS screenshot filenames like `Screenshot 2026-02-13 at 3.54.46 PM.png` → `Screenshot-2026-02-13-at-3-54-46-PM.png`.

Corner cases considered:
- **Hidden/dotfiles** (`.gitignore`): only one dot, it's the last, so preserved. But these aren't uploadable types anyway.
- **Double extensions** (`archive.tar.gz` → `archive-tar.gz`): loses semantic meaning, but `.tar.gz` isn't an uploadable type in this dialog (accepts `image/*,.pdf,.svg`).
- **No extension** (`Makefile`): no dots, nothing to replace.
- **Multiple interior dots** (`my.cool.photo.jpeg` → `my-cool-photo.jpeg`): desirable behavior.

Since the upload dialog only accepts images, PDFs, and SVGs, the problematic corner cases don't arise in practice.

### Interface changes

The `onUploadBinaryFile` callback currently takes just a `File` object. We need to also pass the desired filename. Two options:
- **Option A**: Change signature to `onUploadBinaryFile(file: File, filename: string)`
- **Option B**: Wrap in a new type `{ file: File, targetName: string }`

Option A is simpler and more direct. Go with that.

### UI for filename editing

Each file preview row currently shows: `[thumbnail] [name] [size] [remove button]`

Change to: `[thumbnail] [editable input for name] [size] [remove button]`

The input should:
- Be pre-filled with the sanitized filename (whitespace → `-`, interior dots → `-`)
- Show validation errors inline (same pattern as existing `file-error`)
- Validate against the same rules as text file creation (no `<>:"|?*\` characters, not empty, no duplicates with existing files or other uploads in the batch)

## Work Items

### Phase 1: Sanitization utility

- [x] Add `sanitizeFilename(name: string): string` function to `resourceService.ts`
  - Replaces all Unicode whitespace with `-`
  - Replaces interior dots (all dots except the last) with `-`
  - Collapses consecutive `-` into a single `-`
  - Trims leading/trailing `-` (but not leading `.` for dotfiles)
- [x] Add unit tests for `sanitizeFilename` in `resourceService.test.ts` (17 tests, all passing)

### Phase 2: Dialog UI changes

- [x] Add `editedNames` state to `NewFileDialog` — a `Map<File, string>` tracking user-edited names
- [x] In `processFiles()`, compute sanitized default name for each file and populate `editedNames`
- [x] Replace the read-only `<span className="file-name">` with an `<input>` field
- [x] Add CSS for the inline filename input (compact, fits within the file-preview row)
- [x] Add validation for edited names:
  - No invalid characters (`<>:"|?*\`)
  - Not empty
  - No duplicates with `existingPaths` or other files in the batch
- [x] Show validation errors per-file (reuse existing `file-error` styling)
- [x] Upload button disabled when any file has validation errors
- [x] `editedNames` cleaned up when removing files and when dialog closes

### Phase 3: Plumb edited name through upload chain

- [x] Change `NewFileDialogProps.onUploadBinaryFile` signature from `(file: File) => void` to `(file: File, targetName: string) => void`
- [x] Update `handleUploadFiles()` to pass the edited name from `editedNames` map
- [x] Update `Editor.handleUploadBinaryFile()` to accept and use the `targetName` parameter instead of `file.name`

### Phase 4: Testing

- [x] Verify existing hub-client tests still pass (304/304 pass)
- [x] Typecheck passes
- [ ] Manual testing: drag file with spaces in name, verify sanitized default appears in input
- [ ] Manual testing: edit the filename, verify upload uses edited name
- [ ] Manual testing: verify validation errors show for invalid names
