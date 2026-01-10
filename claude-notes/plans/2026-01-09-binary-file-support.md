# Binary File Support for Quarto-Hub

**Created:** 2026-01-09
**Status:** Planning
**Related Documents:**
- 2025-12-08-quarto-hub-mvp.md (hub architecture)
- 2025-12-11-unified-lsp-hub-design.md (unified architecture)
- 2025-12-22-quarto-hub-web-frontend-and-wasm.md (frontend design, MVP limitations)

## Executive Summary

This plan removes the "MVP limitation" that binary assets are not stored in automerge. It extends the automerge document schema to support binary files (images, PDFs, etc.) alongside text files. This enables:
1. Project images to appear in HTML preview
2. Complete project asset management
3. Drag-and-drop image upload in hub-client
4. Concurrent upload conflict resolution

## Architecture Context

**hub-client** and **quarto-hub** are independent components that communicate via automerge sync protocol:

```
┌──────────────────────────┐              ┌──────────────────────────┐
│      hub-client          │              │       quarto-hub         │
│      (browser SPA)       │              │    (local sync server)   │
│                          │              │                          │
│  ┌────────────────────┐  │   WebSocket  │  ┌────────────────────┐  │
│  │  automerge-repo    │◄─┼──── sync ───►┼──│   automerge docs   │  │
│  │  (JS library)      │  │   protocol   │  └────────────────────┘  │
│  └────────────────────┘  │              │           │              │
│                          │              │           ▼              │
│  ┌────────────────────┐  │              │  ┌────────────────────┐  │
│  │ wasm-quarto-hub-   │  │              │  │    filesystem      │  │
│  │ client (VFS+render)│  │              │  │    (local files)   │  │
│  └────────────────────┘  │              │  └────────────────────┘  │
└──────────────────────────┘              └──────────────────────────┘
```

**Key points:**
- **hub-client** can connect to ANY automerge sync server (quarto-hub, sync.automerge.org, etc.)
- **quarto-hub** is one implementation that additionally backs to the local filesystem
- **No code is shared** between them - wasm-quarto-hub-client is a separate WASM module
- Changes to hub-client (Phases 2-4) work with any sync server
- Changes to quarto-hub (Phase 1) are only needed for local filesystem backing

## Current State Analysis

### Current Index Document Schema
```
ROOT
└── files: Map<String, String>  // path -> document_id (bs58-encoded)
```

### Current File Document Schema (text files)
```
ROOT
└── text: Text  // automerge Text type
```

### Current MVP Limitations (from 2025-12-22 plan)
- Binary assets (images, CSS files) are NOT stored in automerge
- "Future: Binary asset sync strategy TBD (base64 in automerge, external blob storage, etc.)"

### What Already Exists
- **WASM VFS supports binary files**: `vfs_add_binary_file(path, content: &[u8])` in wasm-quarto-hub-client
- **TypeScript wrapper**: `vfsAddBinaryFile(path, content: Uint8Array)` in wasmRenderer.ts

### What's Missing
- Automerge document schema for binary files
- Loading binary files from automerge into VFS
- Serving binary files to preview iframe
- UI for uploading binary files
- quarto-hub filesystem backing for binary files

## Automerge Binary Support

Automerge natively supports `Uint8Array` as a scalar type:

```typescript
import * as A from "@automerge/automerge";

let doc = A.from({
  bytes: new Uint8Array([1, 2, 3]),
});

// Result: { bytes: Uint8Array(3) [1, 2, 3] }
```

This means we can store binary data directly in automerge documents.

---

## Schema Design

### Design Principle: Self-Describing Documents

**Key decision:** The document itself determines whether it's text or binary, not the index.

**Rationale:** This design was chosen over an alternative that used separate `files` and `resources` maps in the index document. The self-describing approach is superior because:

1. **Consistency is local** - To validate a document, you only need to inspect that document. No cross-document coordination required.

2. **Simpler index** - The index remains a simple path→docId map. No need to categorize files at the index level.

3. **Invalid states are locally detectable** - If a document somehow has both `text` and `content` fields, this is immediately visible when loading that one document, rather than requiring comparison between index metadata and document content.

4. **Type belongs with content** - Whether something is text or binary is a property of the content itself, not the reference to it.

### Index Document Schema (Unchanged)

```
ROOT
└── files: Map<String, String>  // path -> document_id (bs58-encoded)
```

The index document structure remains exactly the same. All files (text and binary) are stored in the same `files` map.

### Text File Document Schema (Unchanged)

```
ROOT
└── text: Text  // automerge Text type
```

Existing text files continue to work exactly as before.

### Binary File Document Schema (New)

```
ROOT
├── content: Bytes     // Uint8Array with file contents
├── mimeType: String   // MIME type (e.g., "image/png")
└── hash: String       // SHA-256 hash of content (hex-encoded)
```

**Field descriptions:**
- `content`: The raw binary data as a `Uint8Array`
- `mimeType`: Required for proper rendering and download
- `hash`: Used for collision detection during concurrent uploads and optional deduplication

### Document Validation Rules

A valid file document must satisfy exactly one of:
1. Has `text` field (Text type) at ROOT → text document
2. Has `content` field (Bytes type) at ROOT → binary document

Invalid states:
- Neither `text` nor `content` → empty/invalid document
- Both `text` and `content` → invalid state, should not occur

If an invalid state is encountered (both fields present), the implementation should:
- Log a warning
- Prefer `text` for backwards compatibility, OR
- Treat as error and refuse to load

### Type Detection

**When loading a document:**
```typescript
function getDocumentType(doc: AutomergeDoc): 'text' | 'binary' | 'invalid' {
  const hasText = 'text' in doc;
  const hasContent = 'content' in doc;

  if (hasText && !hasContent) return 'text';
  if (hasContent && !hasText) return 'binary';
  return 'invalid';
}
```

**For UI display (file icons) before loading:**
Infer from file extension as a heuristic:
- Text: `.qmd`, `.yml`, `.yaml`, `.md`, `.txt`, `.json`
- Binary: `.png`, `.jpg`, `.jpeg`, `.gif`, `.svg`, `.pdf`, `.webp`

This heuristic is for display optimization only; the document content is the source of truth.

---

## Concurrent Upload Conflict Resolution

### The Problem

When two users upload images concurrently:
1. User A uploads `diagram.png` (their version)
2. User B uploads `diagram.png` (different content)
3. Both users' clients add to the index simultaneously

### Solution: Content-Addressable Naming

The `hash` field enables conflict detection and resolution:

**For duplicate uploads of same content:**
- Hash matches existing document → can reuse (optional deduplication)
- No conflict because content is identical

**For different content with same filename:**

Automatic renaming (recommended):
```
diagram.png → diagram-a1b2c3d4.png
diagram.png → diagram-e5f6g7h8.png
```

Where `a1b2c3d4` is the first 8 characters of the hex-encoded SHA-256 hash.

**Algorithm:**
1. User initiates upload of `diagram.png`
2. Compute SHA-256 hash of content
3. Check if path `diagram.png` exists in index
   - If no: use original name
   - If yes: check if existing document has same hash
     - Same hash: reuse existing (optional), upload complete
     - Different hash: generate unique name `diagram-{hash8}.png`
4. Create document and add to index

### Automerge Merge Semantics

Since automerge uses last-writer-wins (LWW) for map entries:
- If both users add same path with same docId → converges to single entry ✓
- If both users add same path with different docId → one wins (LWW) ✗

The hash-based naming ensures different content → different paths, avoiding the LWW "loser" scenario entirely.

### Deduplication (Optional, Future Enhancement)

For MVP, skip deduplication. If user uploads same image twice, they get two documents. This is simple and correct.

Future enhancement: maintain an in-memory hash→docId cache by scanning loaded documents. This enables efficient duplicate detection without changing the schema.

---

## File Management UI Redesign

### Current Issues
1. "+" button doesn't work
2. No drag-and-drop support
3. No file renaming
4. No file deletion UI

### Proposed UI Changes

#### File List Sidebar
```
┌────────────────────────────────┐
│ Files                      [+] │
├────────────────────────────────┤
│ 📄 _quarto.yml                │
│ 📄 index.qmd           ← active│
│ 📄 about.qmd                  │
│ 📁 images/                    │
│   🖼️ diagram.png              │
│   🖼️ logo.svg                 │
└────────────────────────────────┘
```

Features:
- Tree view for nested directories
- Icons distinguish file types (📄 text, 🖼️ image, etc.)
- Context menu (right-click) for: Rename, Delete, Download
- Active file highlighting

#### New File Dialog
```
┌─────────────────────────────────────┐
│ Create New File                     │
├─────────────────────────────────────┤
│ Filename: [_______________]         │
│                                     │
│ ─── Or drag & drop an image ───     │
│ ┌─────────────────────────────────┐ │
│ │                                 │ │
│ │  Drop image here or click to    │ │
│ │  browse                         │ │
│ │                                 │ │
│ └─────────────────────────────────┘ │
│                                     │
│           [Cancel] [Create]         │
└─────────────────────────────────────┘
```

#### Drag & Drop Implementation

1. **Drop zone in dialog**: Primary way to add images
2. **Drop zone in editor**: Drop image to insert `![](path)` reference
3. **Progress indicator**: Show upload progress for large files

---

## Implementation Phases

### Phase 1: Filesystem Backing for Binary Files (quarto-hub, Rust)

**Purpose:** Enable quarto-hub to sync binary files between automerge documents and the local filesystem. This phase is only needed when using quarto-hub as the sync server with local file backing. Hub-client changes (Phases 2-4) work independently with any sync server.

**Changes to existing modules:**

1. **index.rs** - Minimal changes:
   - No structural changes (same `files` map)
   - Possibly add helper method to distinguish document types when loading

2. **New module: resource.rs**
   - `create_binary_document(content: &[u8], mime_type: &str) -> Automerge`
   - SHA-256 hash computation
   - MIME type detection (use `infer` or `mime_guess` crate)

3. **discovery.rs** - Extend file discovery:
   - Add image extensions: `.png`, `.jpg`, `.jpeg`, `.gif`, `.svg`, `.webp`
   - Add other binary types: `.pdf`
   - Keep distinction for internal use (which files to load as text vs binary)

4. **sync.rs** - Handle binary sync:
   - Detect document type when syncing to filesystem
   - Text documents: write as UTF-8 text
   - Binary documents: write raw bytes
   - Reading: infer type from extension, create appropriate document

5. **context.rs** - Reconcile binary files on startup:
   - When creating index document, discover and add binary files
   - When automerge changes arrive, write binary files to disk
   - When filesystem changes (watcher), update automerge documents

**New dependencies:**
- `sha2` crate for SHA-256 hashing
- `infer` or `mime_guess` for MIME type detection

### Phase 2: Automerge Binary Document Support (hub-client, TypeScript)

**Purpose:** Enable hub-client to create, load, and sync binary documents via automerge. Works with any automerge sync server.

1. **Update types/project.ts:**
   ```typescript
   // Existing
   interface FileEntry {
     path: string;
     docId: string;
   }

   // New - document content types
   type TextDocument = { text: string };
   type BinaryDocument = {
     content: Uint8Array;
     mimeType: string;
     hash: string;
   };
   type FileDocument = TextDocument | BinaryDocument;

   function isTextDocument(doc: FileDocument): doc is TextDocument {
     return 'text' in doc;
   }

   function isBinaryDocument(doc: FileDocument): doc is BinaryDocument {
     return 'content' in doc;
   }
   ```

2. **Update automergeSync.ts:**
   - Handle loading documents with `content` field
   - Expose binary content to UI layer
   - Add upload function for binary content

3. **New service: resourceService.ts**
   ```typescript
   async function uploadBinaryFile(
     file: File
   ): Promise<{ docId: string; path: string }> {
     const content = await file.arrayBuffer();
     const hash = await computeSHA256(content);
     const mimeType = file.type || inferMimeType(file.name);
     // ... create document, handle naming conflicts
   }

   async function computeSHA256(data: ArrayBuffer): Promise<string> {
     const hashBuffer = await crypto.subtle.digest('SHA-256', data);
     return Array.from(new Uint8Array(hashBuffer))
       .map(b => b.toString(16).padStart(2, '0'))
       .join('');
   }
   ```

### Phase 3: VFS Integration for Preview

**Purpose:** Wire binary files from automerge documents into the WASM VFS and serve them to the preview iframe.

**Note:** The WASM VFS already supports binary files via `vfs_add_binary_file()`. The work here is:
1. Populating the VFS when binary documents are loaded from automerge
2. Serving binary content to the preview iframe

For images to appear in the HTML preview iframe:

1. **Update automergeSync.ts to populate VFS with binary files:**
   - When loading binary documents, call `vfsAddBinaryFile(path, content)`
   - Subscribe to binary document changes (though binary docs don't change often)

2. **Extend iframe post-processor (useIframePostProcessor.ts):**
   - Find `<img>` elements with project-relative `src`
   - Read binary content from VFS
   - Convert to data URL: `data:{mimeType};base64,{content}`
   - Replace `src` attribute

   ```typescript
   // Similar to existing CSS handling
   doc.querySelectorAll('img').forEach((img) => {
     const src = img.getAttribute('src');
     if (src && !src.startsWith('data:') && !src.startsWith('http')) {
       const binary = vfsReadBinaryFile(src);
       if (binary) {
         const base64 = btoa(String.fromCharCode(...binary));
         const mimeType = inferMimeType(src);
         img.setAttribute('src', `data:${mimeType};base64,${base64}`);
       }
     }
   });
   ```

3. **Handle image formats:**
   - PNG, JPEG, GIF, WebP: standard base64 data URLs
   - SVG: can use `data:image/svg+xml;base64,...` or inline

### Phase 4: File Management UI

1. **File sidebar component:**
   - Tree view with folder expansion
   - File type icons (inferred from extension)
   - Context menu with rename/delete/download
   - Highlight active file

2. **New file dialog component:**
   - Text input for filename
   - Drag & drop zone for images
   - File browser fallback (click to browse)
   - Validation (allowed extensions, size limits)

3. **Drag & drop handlers:**
   - `onDrop` on sidebar → opens dialog with file pre-filled
   - `onDrop` on editor → uploads image, inserts markdown reference

4. **Fix existing "+" button:**
   - Wire up to open new file dialog

### Phase 5: File Rename Support

1. **Backend (index.rs or via sync):**
   - Rename = update key in `files` map
   - Document content unchanged (same docId)

2. **Client (automergeSync.ts):**
   ```typescript
   function renameFile(oldPath: string, newPath: string): void {
     const docId = indexDoc.files[oldPath];
     delete indexDoc.files[oldPath];
     indexDoc.files[newPath] = docId;
   }
   ```

3. **UI:**
   - Context menu "Rename" option
   - Inline editing in file tree (double-click or F2)
   - Validation: no duplicate paths, valid characters

---

## Technical Decisions

### Size Limits

Recommended limits:
- Single file: 10 MB max
- Total project resources: 100 MB (revisit based on performance)

Rationale: Automerge sync works well for moderate-sized documents. Very large files should use external storage.

### MIME Type Detection

Use libraries for reliable detection:
- **Rust:** `infer` crate (magic bytes detection)
- **TypeScript:** Browser's `File.type` property, fall back to extension

Don't rely solely on file extension - detect from content when possible.

### Hash Algorithm

SHA-256:
- Fast enough for file hashing
- Universally available (Rust `sha2`, Web Crypto API)
- 64-char hex string, 8-char prefix for filenames

---

## Backwards Compatibility

**This design is fully backwards compatible:**

1. Index structure unchanged (`files` map)
2. Existing text documents unchanged (`text` field)
3. Binary support is purely additive
4. Old clients that don't understand binary will:
   - See binary files in index (paths appear)
   - Fail gracefully when loading (no `text` field)
   - Not corrupt the documents

**No migration required** for existing projects.

---

## Communication Architecture

**No REST API** - hub-client is a static web asset that communicates entirely via automerge sync protocol over WebSocket.

### Binary File Upload Flow

1. User drops image in hub-client browser
2. hub-client creates automerge document: `{ content: Uint8Array, mimeType, hash }`
3. hub-client adds path→docId mapping to index document
4. Changes sync via automerge protocol to all connected peers
5. quarto-hub (if connected) receives sync, writes file to local disk

### Binary File Access for Preview

1. hub-client loads binary document via automerge sync
2. Content stored in browser-side VFS
3. iframe post-processor converts to data URLs (same pattern as CSS handling)

### Sync Protocol

No changes needed - automerge sync protocol handles `Uint8Array` values automatically.

---

## Open Questions

1. **Should we support editing binary files?**
   - Current answer: No, binary files are immutable
   - Editing = delete old + upload new

2. **How to handle very large files (>10MB)?**
   - Option: External storage with URL reference
   - Defer to future enhancement

3. **Should renaming update references in .qmd files?**
   - Could scan for `![...](old-path)` and update
   - Adds significant complexity
   - Recommendation: Defer, users can find/replace manually

4. **Folder creation?**
   - Implicit (created when file with nested path is added)
   - Recommendation: Implicit for simplicity

---

## Success Metrics

1. Can upload image via drag-and-drop
2. Image appears in HTML preview
3. Two users can concurrently upload images without data loss
4. Files can be renamed and deleted
5. Existing projects continue to work without migration

---

## Estimated Complexity

| Phase | Description | Complexity |
|-------|-------------|------------|
| 1 | Backend Schema Support | Small-Medium |
| 2 | Client Schema Support | Medium |
| 3 | VFS Integration | Medium |
| 4 | File Management UI | Large |
| 5 | File Rename Support | Small |

**Note:** Phase 1 is simpler than originally estimated because the index structure doesn't change.

Recommended order: 1 → 2 → 3 → 4 → 5

---

## Build Instructions

### Building wasm-quarto-hub-client

**Recommended method** - use the hub-client build script:

```bash
cd hub-client
npm run build:wasm
```

This script (`scripts/build-wasm.js`) handles:
- Finding Homebrew LLVM with wasm32 support on macOS
- Setting proper CFLAGS for the wasm-sysroot
- Using wasm-pack for proper wasm-bindgen integration

**Manual method** (if not using npm):

```bash
cd crates/wasm-quarto-hub-client
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"  # macOS Apple Silicon
export CFLAGS_wasm32_unknown_unknown="-I$(pwd)/wasm-sysroot -Wbad-function-cast -Wcast-function-type -fno-builtin -DHAVE_ENDIAN_H"
wasm-pack build --target web
```

**Important:** The system clang doesn't support wasm32 targets - you need Homebrew's LLVM which has wasm support. Do NOT use `cargo build` directly without wasm-pack and the proper LLVM in PATH.

---

## References

- [Automerge Document Data Model](https://automerge.org/docs/reference/documents/)
- [Automerge byte arrays](https://automerge.org/docs/reference/documents/) - Uint8Array support
- [Web Crypto API - SHA-256](https://developer.mozilla.org/en-US/docs/Web/API/SubtleCrypto/digest)
- [File API](https://developer.mozilla.org/en-US/docs/Web/API/File)
- [Data URLs](https://developer.mozilla.org/en-US/docs/Web/HTTP/Basics_of_HTTP/Data_URLs)
