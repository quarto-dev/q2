/**
 * Agent-task definitions for the ERG-7 eval suite (Phase 0,
 * bd-f1dr7gs1; claude-notes/plans/2026-10-05-elevate-quarto-hub-mcp.md).
 *
 * Each task is a realistic agent job scripted against the in-process
 * test-hub, with a machine-checkable success condition evaluated against
 * hub state (not the agent's prose, except where the deliverable IS
 * prose). The runner (run.mjs) supplies `ctx`:
 *
 *   ctx = {
 *     hub,                    // TestHub — server-side ground truth
 *     manager,                // ConnectionManager bound to the hub
 *     seedProject(files),     // → { indexDocId } after confirmed delivery
 *     readFile(id, path),     // → text | undefined (hub-side view)
 *     listPaths(id),          // → sorted paths (hub-side view)
 *     editFile(id, path, text),  // collaborator edit via the runner's client
 *   }
 *
 * Task shape:
 *   id        — stable slug (used in result files and the plan baseline)
 *   prompt    — (ctx) → the exact prompt handed to `claude -p`
 *   setup     — (ctx) → seeds hub state before the agent starts
 *   check     — (ctx, finalText) → true/false; run after the agent exits
 *   onTick?   — (ctx, elapsedMs) → called once a second while the agent
 *               runs; the live-watch task uses it to land a collaborator
 *               edit mid-run.
 *
 * Determinism notes: tasks never depend on the agent's exact tool
 * sequence, only on the resulting hub state and final text. The
 * live-watch task's collaborator edit is deliberately late-bound
 * (onTick) so a slow agent still sees it as a *new* change; a fallback
 * read also answers correctly, which is fine — we score task success,
 * not tool choice.
 */

/** Base58 automerge index doc id, bare or inside a share URL. The sync
 * client's index doc ids are 28 chars today (16-byte bs58); allow slack
 * for alphabet/length changes without matching ordinary words. */
export function extractProjectId(text) {
  const share = text.match(/#\/share\/([1-9A-HJ-NP-Za-km-z]{24,44})/);
  if (share) return share[1];
  const bare = text.match(/\b[1-9A-HJ-NP-Za-km-z]{24,44}\b/);
  return bare ? bare[0] : undefined;
}

const MCP_ONLY =
  'Use only the Quarto Hub MCP tools (no shell, no local filesystem). ';

export const TASKS = [
  {
    id: 'create-project',
    prompt: () =>
      MCP_ONLY +
      'Create a new Quarto Hub project with two files: `index.qmd` whose ' +
      "YAML front matter sets the title 'Eval Home' followed by a short " +
      'paragraph, and `_quarto.yml` declaring a default project. Then ' +
      'reply with ONLY the project id (the automerge index document id).',
    setup: async () => {},
    check: async (ctx, finalText) => {
      const id = extractProjectId(finalText);
      if (!id) return false;
      const text = await ctx.readFile(id, 'index.qmd');
      return text !== undefined && text.includes('Eval Home');
    },
  },

  {
    id: 'read-and-report',
    prompt: (ctx) =>
      MCP_ONLY +
      `Connect to the Quarto Hub project ${ctx.projectId} and tell me the ` +
      "exact document title set in index.qmd's YAML front matter.",
    setup: async (ctx) => {
      const { indexDocId } = await ctx.seedProject([
        {
          path: 'index.qmd',
          content: '---\ntitle: Quarterly Frobnication Report\n---\n\nBody text.\n',
        },
      ]);
      ctx.projectId = indexDocId;
    },
    check: async (_ctx, finalText) => finalText.includes('Quarterly Frobnication Report'),
  },

  {
    id: 'patch-typo',
    prompt: (ctx) =>
      MCP_ONLY +
      `In Quarto Hub project ${ctx.projectId}, the file intro.qmd contains ` +
      "the typo 'teh'. Fix it to 'the'. Change nothing else.",
    setup: async (ctx) => {
      const { indexDocId } = await ctx.seedProject([
        { path: 'intro.qmd', content: 'We must teh frobnicate before dawn.\n' },
      ]);
      ctx.projectId = indexDocId;
    },
    check: async (ctx) =>
      (await ctx.readFile(ctx.projectId, 'intro.qmd')) ===
      'We must the frobnicate before dawn.\n',
  },

  {
    id: 'write-new-file',
    prompt: (ctx) =>
      MCP_ONLY +
      `In Quarto Hub project ${ctx.projectId}, create a new file ` +
      'shopping.md containing a short bullet list of three items (your ' +
      'choice of items).',
    setup: async (ctx) => {
      const { indexDocId } = await ctx.seedProject([
        { path: 'index.qmd', content: '---\ntitle: Home\n---\n' },
      ]);
      ctx.projectId = indexDocId;
    },
    check: async (ctx) => {
      const text = await ctx.readFile(ctx.projectId, 'shopping.md');
      if (text === undefined) return false;
      return text.split('\n').filter((l) => l.trim().length > 0).length >= 3;
    },
  },

  {
    id: 'rename-file',
    prompt: (ctx) =>
      MCP_ONLY +
      `In Quarto Hub project ${ctx.projectId}, rename the file draft.qmd ` +
      'to published.qmd, keeping its content.',
    setup: async (ctx) => {
      const { indexDocId } = await ctx.seedProject([
        { path: 'draft.qmd', content: '---\ntitle: Draft\n---\n\nKeep me.\n' },
      ]);
      ctx.projectId = indexDocId;
    },
    check: async (ctx) => {
      const paths = await ctx.listPaths(ctx.projectId);
      return (
        !paths.includes('draft.qmd') &&
        paths.includes('published.qmd') &&
        (await ctx.readFile(ctx.projectId, 'published.qmd'))?.includes('Keep me.')
      );
    },
  },

  {
    id: 'collaborator-edit',
    prompt: (ctx) =>
      MCP_ONLY +
      `A collaborator just edited status.qmd in Quarto Hub project ` +
      `${ctx.projectId}. Read it and tell me the deploy status it now reports.`,
    setup: async (ctx) => {
      const { indexDocId } = await ctx.seedProject([
        { path: 'status.qmd', content: 'deploy: pending\n' },
      ]);
      ctx.projectId = indexDocId;
      // The collaborator edit lands before the agent starts (its MCP
      // server connects fresh and must see current state).
      await ctx.editFile(indexDocId, 'status.qmd', 'deploy: shipped\n');
    },
    check: async (_ctx, finalText) => finalText.includes('shipped'),
  },

  {
    id: 'watch-live-edit',
    prompt: (ctx) =>
      MCP_ONLY +
      `In Quarto Hub project ${ctx.projectId}, use wait_for_change to ` +
      'watch the file live.qmd for the next collaborator edit, then report ' +
      'the status the file contains after that edit. Keep watching (re-call ' +
      'wait_for_change if it times out) until the edit arrives.',
    setup: async (ctx) => {
      const { indexDocId } = await ctx.seedProject([
        { path: 'live.qmd', content: 'status: building\n' },
      ]);
      ctx.projectId = indexDocId;
      ctx._editLanded = false;
    },
    onTick: async (ctx, elapsedMs) => {
      // Land the collaborator edit ~45 s in: late enough that the agent
      // has booted, connected, and is plausibly watching; early enough
      // that a re-watch still catches the aftermath.
      if (!ctx._editLanded && elapsedMs >= 45000) {
        ctx._editLanded = true;
        await ctx.editFile(ctx.projectId, 'live.qmd', 'status: deployed\n');
      }
    },
    check: async (_ctx, finalText) => finalText.includes('deployed'),
  },

  {
    // Error-recovery (Phase 1, ERG-4/ERG-1): the agent's write is
    // REFUSED on a stale expected_hash — by construction, since the
    // prompt embeds a pre-collaboration hash — and it must recover
    // (re-read, merge, retry) without losing anyone's content. One
    // isError result is expected here; that is the point of the task.
    id: 'stale-write-recovery',
    prompt: (ctx) =>
      MCP_ONLY +
      `In Quarto Hub project ${ctx.projectId}, update the file notes.qmd ` +
      'by appending the line `Line two.` on a new line. You previously ' +
      "read this file (its content was exactly 'Line one.\\n', hash " +
      `${ctx.staleHash}) — use that hash as expected_hash on your write ` +
      'so a collaborator edit can never be clobbered. If the write is ' +
      'refused, recover: re-read the file, keep EVERY line already ' +
      'there, and retry so no content is lost. The final file must ' +
      'contain every pre-existing line plus `Line two.`',
    setup: async (ctx) => {
      const { createHash } = await import('node:crypto');
      const stale = 'Line one.\n';
      ctx.staleHash =
        'sha256:' + createHash('sha256').update(stale, 'utf8').digest('hex');
      const { indexDocId } = await ctx.seedProject([
        { path: 'notes.qmd', content: stale },
      ]);
      ctx.projectId = indexDocId;
      // The collaborator edit lands before the agent starts, so the
      // prompt-embedded hash is deterministically stale.
      await ctx.editFile(indexDocId, 'notes.qmd', 'Line one.\nCollaborator was here.\n');
    },
    check: async (ctx) => {
      const text = await ctx.readFile(ctx.projectId, 'notes.qmd');
      return (
        text !== undefined &&
        text.includes('Collaborator was here.') &&
        text.includes('Line two.')
      );
    },
  },

  // Phase 2 (bd-oqp2kva8): binary write through write_file's
  // `encoding: "base64"` — the agent must discover the parameter from
  // the tool schema and the bytes must land bit-exact on the hub.
  {
    id: 'add-image-binary',
    prompt: (ctx) =>
      MCP_ONLY +
      `In Quarto Hub project ${ctx.projectId}, create the file ` +
      '`images/logo.png` containing exactly this PNG image (base64): ' +
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk' +
      '+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg== . ' +
      'It must be a real binary file, not the base64 text. Reply DONE ' +
      'when the file is in the project.',
    setup: async (ctx) => {
      const { indexDocId } = await ctx.seedProject([
        { path: 'index.qmd', content: '# Home\n' },
      ]);
      ctx.projectId = indexDocId;
    },
    check: async (ctx) => {
      // Byte-exactness against hub-side ground truth.
      const indexHandle = await ctx.hub.repo.find(ctx.projectId);
      const index = indexHandle.doc();
      const docId = index?.files?.['images/logo.png'];
      if (!docId) return false;
      const binId = String(docId).startsWith('automerge:')
        ? String(docId)
        : `automerge:${docId}`;
      const binHandle = await ctx.hub.repo.find(binId);
      const doc = binHandle.doc();
      if (!doc?.content || doc.mimeType !== 'image/png') return false;
      const expected = Buffer.from(
        'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk' +
          '+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==',
        'base64',
      );
      return Buffer.from(doc.content).equals(expected);
    },
  },

  // Phase 2 (bd-oqp2kva8): project-wide content search. Scored on the
  // answer, not on the agent picking search_files.
  {
    id: 'search-and-report',
    prompt: (ctx) =>
      MCP_ONLY +
      `In Quarto Hub project ${ctx.projectId}, find which file mentions ` +
      "the word 'spindle' and the 1-based line it appears on. " +
      'Reply with ONLY the answer in the form path:line.',
    setup: async (ctx) => {
      const { indexDocId } = await ctx.seedProject([
        { path: 'index.qmd', content: '---\ntitle: Loom\n---\n\nNothing here.\n' },
        {
          path: 'notes/deep.qmd',
          content: 'First line.\nSecond line.\nThe spindle turns quietly.\nFourth line.\n',
        },
        { path: 'notes/other.qmd', content: 'Unrelated content.\n' },
      ]);
      ctx.projectId = indexDocId;
    },
    check: async (_ctx, finalText) => /notes\/deep\.qmd:3\b/.test(finalText.trim()),
  },

  // Phase 3 (bd-3qe7unp7): project-wide watch. The prompt names no
  // tool; the agent must discover that wait_for_change with no `path`
  // watches the whole project, and answer from its result.
  {
    id: 'watch-project-edit',
    prompt: (ctx) =>
      MCP_ONLY +
      `In Quarto Hub project ${ctx.projectId}, watch the WHOLE project ` +
      '(not any one file) for the next collaborator change, then report ' +
      'which file changed and the status line it contains after the ' +
      'change. Keep watching (re-call the tool if it times out) until a ' +
      'change arrives.',
    setup: async (ctx) => {
      const { indexDocId } = await ctx.seedProject([
        { path: 'one.qmd', content: 'status: quiet\n' },
        { path: 'two.qmd', content: 'status: idle\n' },
      ]);
      ctx.projectId = indexDocId;
      ctx._editLanded = false;
    },
    onTick: async (ctx, elapsedMs) => {
      if (!ctx._editLanded && elapsedMs >= 45000) {
        ctx._editLanded = true;
        await ctx.editFile(ctx.projectId, 'two.qmd', 'status: deployed\n');
      }
    },
    check: async (_ctx, finalText) =>
      finalText.includes('two.qmd') && finalText.includes('deployed'),
  },

  // Phase 3 (bd-3qe7unp7): history-driven undo. A bad edit lands before
  // the agent starts; it must find the prior version via project
  // history and put the file back. Scored on hub state, not tool choice.
  {
    id: 'history-and-restore',
    prompt: (ctx) =>
      MCP_ONLY +
      `In Quarto Hub project ${ctx.projectId}, the file deploy.qmd was ` +
      'just damaged by a bad automated edit — it should say ' +
      "'status: green'. Investigate what the file contained before the " +
      'bad edit (use whatever project history the tools expose) and put ' +
      'the file back to that earlier good state, without losing the ' +
      "record of what happened. When done, reply with the file's " +
      'restored status line.',
    setup: async (ctx) => {
      const { indexDocId } = await ctx.seedProject([
        { path: 'deploy.qmd', content: 'status: green\n' },
      ]);
      ctx.projectId = indexDocId;
      // The damaging edit lands before the agent starts.
      await ctx.editFile(indexDocId, 'deploy.qmd', 'status: CORRUPTED x9z\n');
    },
    check: async (ctx, _finalText) =>
      (await ctx.readFile(ctx.projectId, 'deploy.qmd')) === 'status: green\n',
  },
];
