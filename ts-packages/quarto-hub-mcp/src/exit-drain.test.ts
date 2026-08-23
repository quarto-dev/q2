/**
 * Exit-drain at the stdio level (bd-10deu8h4) — the exact 2026-06-12
 * accident, replayed against the real server binary.
 *
 * The incident: a `q2 mcp` session ran `create_project`, read the file
 * back from process memory, and the host closed stdin. The stdin-EOF
 * shutdown (bd-9jq2a060, correct behavior) ran `disconnectAll()` and
 * exited before the new file document's sync to the hub completed. The
 * index entry escaped; the file document's only copy died with the
 * process (MCP clients use memory storage) — a dangling index entry
 * that bricked the project for every client.
 *
 * Contract under test: shutdown drains outbound sync (bounded) before
 * `process.exit`, while preserving the prompt stdin-EOF exit that
 * stdio-hygiene.test.ts asserts. Ground truth is the hub's own repo.
 */

import { describe, it, expect, beforeAll, afterAll, afterEach } from 'vitest';

import { McpTestClient } from './mcp-test-client.js';
import { startTestHub, type TestHub } from './test-hub.js';

// Enough payload to keep delivery genuinely in flight when stdin
// closes right after the tool result — a single tiny file wins the
// race by luck on loopback, masking the defect. Both levers matter:
// many docs (per-doc synchronizer backlog) and real bytes (encode +
// send time).
//
// Do not shrink this to make the test faster. The payload is what
// makes the assertion bind, and the floor is high. Measured on an idle
// 12-core M-series Mac with FILE_COUNT fixed at 64 (bd-yw3mcdkg):
//
//   payload             drain-to-exit   still red with the drain off?
//   64 x 64 KB (4 MB)   1980-2028 ms    yes
//   64 x 48 KB (3 MB)   1478-1490 ms    only sometimes
//   64 x 32 KB (2 MB)        —          NO — passes with no drain at all
//   64 x 16 KB (1 MB)    506- 612 ms    NO — passes with no drain at all
//
// Below ~3 MB everything is delivered before stdin even closes, so the
// drain has nothing left to do and this test would pass against a
// server that never drains — i.e. it would stop testing anything. That
// is measured, not assumed: the drain was disabled and the test re-run
// at each size above.
const FILE_COUNT = 64;
const FILE_BYTES = 64 * 1024;

// The flip side of that floor: ~4 MB does not reliably clear the
// production 3000 ms drain budget on a loaded 3-core CI runner (the
// same payload that drains in ~2.0 s idle took 2353-2808 ms with this
// machine merely busy), which is how this test failed on macOS in 2 of
// 5 runs after PR #579 wired the suite into CI — ubuntu never failed.
//
// So this test overrides the budget rather than racing it. The default
// stays 3000 ms for real sessions and is asserted elsewhere:
// stdio-hygiene.test.ts pins the prompt stdin-EOF exit with live sync
// connections, and the second test below exercises the case where the
// budget actually binds (hub gone). This test's own subject is
// narrower — no created document may be lost — so it should not also
// be a throughput benchmark.
//
// Two alternatives were tried and rejected on measurement: shrinking
// the payload stops the test binding (table above), and `retry` lets it
// pass even with the drain deleted, because the drain-off failure is
// probabilistic and one of three attempts gets through.
const DRAIN_BUDGET_MS = 30_000;

function accidentFiles() {
  return Array.from({ length: FILE_COUNT }, (_, i) => ({
    path: `q2-mcp-hello-${i}.qmd`,
    content: `file ${i}\n${'x'.repeat(FILE_BYTES)}\n`,
  }));
}

interface CreateProjectResponse {
  indexDocId: string;
  files: Array<{ path: string; docId: string }>;
}

describe('exit drains outbound sync (bd-10deu8h4)', () => {
  let hub: TestHub;
  let client: McpTestClient;

  beforeAll(async () => {
    hub = await startTestHub();
  });

  afterAll(async () => {
    await hub.stop();
  });

  afterEach(async () => {
    await client.stop();
  });

  it('create_project then immediate stdin EOF must not lose the created docs', async () => {
    client = new McpTestClient();
    await client.start(['--server', hub.url], {
      envOverrides: { QUARTO_MCP_SHUTDOWN_DRAIN_MS: String(DRAIN_BUDGET_MS) },
    });

    const result = await client.callTool('create_project', {
      files: accidentFiles(),
    });
    const created = JSON.parse(result.content[0]!.text) as CreateProjectResponse;
    expect(created.files).toHaveLength(FILE_COUNT);

    // The exact accident: the host closes stdin right after the tool
    // call returns. The server must still exit — the drain is
    // event-driven and returns the moment the hub confirms, so this
    // lands in ~2 s despite the raised budget. The bound here is a
    // hang-detector, not a promptness assertion: prompt exit under the
    // production 3000 ms budget is stdio-hygiene.test.ts's job, and
    // pinning 5 s here as well is what made this test a throughput race.
    expect(await client.endStdinAndWaitForExit(DRAIN_BUDGET_MS + 5000)).toBe(true);

    // …but not before the created documents reached the hub.
    expect(
      await hub.hubHasDoc(created.indexDocId),
      'index doc must reach the hub',
    ).toBe(true);
    for (const f of created.files) {
      expect(
        await hub.hubHasDoc(f.docId),
        `file doc for ${f.path} must reach the hub — its only copy was in-process`,
      ).toBe(true);
    }
  }, 30000);

  it('warns loudly on stderr (and still exits promptly) when the hub is gone at shutdown', async () => {
    // Own hub — this test kills it mid-session.
    const doomedHub = await startTestHub();
    client = new McpTestClient();
    await client.start(['--server', doomedHub.url]);

    const result = await client.callTool('create_project', {
      files: [{ path: 'survives.qmd', content: 'created while the hub was up\n' }],
    });
    const created = JSON.parse(result.content[0]!.text) as CreateProjectResponse;

    // Hub dies; a file is created during the outage. Its only copy is
    // now in this server process's memory.
    await doomedHub.stop();
    await client.callTool('create_file', {
      project: created.indexDocId,
      path: 'doomed.qmd',
      content: 'created while the hub was down\n',
    });

    // Shutdown must not hang past the drain budget (3 s < the 5 s
    // promptness contract)…
    expect(await client.endStdinAndWaitForExit(5000)).toBe(true);

    // …and must name the project and the possibly-lost path on stderr.
    const warning = client.stderrLines.find((l) =>
      l.includes('Possibly NOT delivered'),
    );
    expect(warning, 'shutdown must warn about undelivered documents').toBeDefined();
    expect(warning).toContain(created.indexDocId);
    expect(warning).toContain('doomed.qmd');
  }, 30000);
});
