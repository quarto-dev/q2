export const meta = {
  name: 'nightly-autofix',
  description: 'Gate open q2 issues for unattended fixing, open PRs for the ones that pass, and repair red autofix PRs',
  whenToUse: 'Run via /nightly-issue-sweep, usually unattended overnight. args: {root, date, maxFixes?, dryRun?, issues?, gateLimit?, maxAttempts?, softFloorGb?, hardFloorGb?, maxTargetGb?}',
  phases: [
    { title: 'Housekeeping', detail: 'reset .worktrees/autofix, prune merged branches, cargo clean when disk is low' },
    { title: 'Repair', detail: 'one fix attempt per red or conflicting autofix PR' },
    { title: 'Select', detail: 'deterministic pre-filter of open issues' },
    { title: 'Gate', detail: 'gate-only verdict per candidate; comments on intrinsic failures' },
    { title: 'Fix', detail: 'sequential sweep-mode fixes, no CI watching' },
  ],
}

// Keep in sync with "Gate version" in autonomously-fix-small-issue/SKILL.md.
const GATE_VERSION = 1

const A = args || {}
const ROOT = A.root
if (!ROOT) throw new Error('args.root (absolute path of the main q2 checkout) is required')
const MAX_FIXES = A.maxFixes ?? 5
const MAX_ATTEMPTS = A.maxAttempts ?? 3
const DRY = !!A.dryRun
const WT = `${ROOT}/.worktrees/autofix`
const SKILL = `${ROOT}/.claude/skills/autonomously-fix-small-issue/SKILL.md`
const SCRIPTS = `${ROOT}/.claude/skills/nightly-issue-sweep/scripts`

const COMMON = `You are one step of the unattended nightly-autofix workflow for quarto-dev/q2 (the user is away; never ask questions — decide, or stop and report).
Main checkout: ${ROOT}. Shared work checkout: ${WT}. Ignore the "# Task Context" block in CLAUDE.local.md; it is the user's own work, not yours.
Never modify the main checkout's working tree, and never touch other .worktrees/* directories. Prefix shell commands with \`cd <abs-path> &&\`, because your working directory may not persist between commands.`

// ---------- schemas ----------
const HK = {
  type: 'object',
  properties: {
    abort: { type: 'boolean' }, abort_reason: { type: 'string' },
    free_gb: { type: 'number' }, target_gb: { type: 'number' }, cleaned: { type: 'boolean' },
    orphan_stash: { type: 'string' }, pruned_branches: { type: 'array', items: { type: 'string' } },
    others: { type: 'array', items: { type: 'object', properties: { path: { type: 'string' }, gb: { type: 'number' } } } },
    error: { type: 'string', description: 'set if the script failed; abort should then be true' },
  },
  required: ['abort', 'free_gb', 'cleaned'],
}
const ISSUE = { type: 'object', properties: { number: { type: 'integer' }, title: { type: 'string' }, reason: { type: 'string' } }, required: ['number', 'reason'] }
const SELECT = { type: 'object', properties: { candidates: { type: 'array', items: ISSUE }, skipped: { type: 'array', items: ISSUE } }, required: ['candidates', 'skipped'] }
const GATE = {
  type: 'object',
  properties: {
    verdict: { type: 'string', enum: ['pass', 'fail-intrinsic', 'fail-environment'] },
    failed_checks: { type: 'array', items: { type: 'string' } },
    summary: { type: 'string', description: 'one or two sentences: what the fix would be (pass) or why it failed' },
    estimated_files: { type: 'integer' },
    comment_posted: { type: 'boolean' },
    comment_text: { type: 'string', description: 'the comment body you posted, or would have posted in dry-run' },
  },
  required: ['verdict', 'summary', 'comment_posted'],
}
const FIX = {
  type: 'object',
  properties: {
    status: { type: 'string', enum: ['pr_opened', 'gate_failed', 'stopped'] },
    pr_url: { type: 'string' }, strand: { type: 'string' }, branch: { type: 'string' },
    summary: { type: 'string', description: 'behaviour change, or why you stopped' },
    local_checks: { type: 'string', description: 'checks run locally and their results' },
  },
  required: ['status', 'summary'],
}
const PRLIST = { type: 'array', items: { type: 'object', properties: { number: { type: 'integer' }, title: { type: 'string' }, head: { type: 'string' }, state: { type: 'string' }, attempts: { type: 'integer' } } } }
const PRS = {
  type: 'object',
  properties: { needs_repair: PRLIST, pending: PRLIST, green: PRLIST, needs_human: PRLIST },
  required: ['needs_repair', 'pending', 'green', 'needs_human'],
}
const REPAIR = {
  type: 'object',
  properties: {
    outcome: { type: 'string', enum: ['pushed_fix', 'merged_main', 'reran_flaky', 'no_change', 'needs_human'] },
    summary: { type: 'string' },
  },
  required: ['outcome', 'summary'],
}

// ---------- steps ----------
const report = { gateVersion: GATE_VERSION, dryRun: DRY, housekeeping: [], repairs: [], gates: [], fixes: [], skipped: [], notes: [] }
let hkCount = 0

async function housekeeping(reportOthers) {
  hkCount++
  const r = await agent(`${COMMON}
Run exactly this and return its JSON output as structured data (copy the fields; do not interpret):
  cd ${ROOT} && ${SCRIPTS}/housekeeping.sh${reportOthers ? ' --report-others' : ''}${A.softFloorGb ? ` --soft-floor-gb ${A.softFloorGb}` : ''}${A.hardFloorGb ? ` --hard-floor-gb ${A.hardFloorGb}` : ''}${A.maxTargetGb ? ` --max-target-gb ${A.maxTargetGb}` : ''}
It can take several minutes when it runs cargo clean (use a long timeout). If it exits non-zero, set abort=true and put the stderr tail in "error".`,
    { label: `housekeeping #${hkCount}`, phase: 'Housekeeping', schema: HK, effort: 'low' })
  const hk = r || { abort: true, abort_reason: 'housekeeping agent died', free_gb: -1, cleaned: false }
  report.housekeeping.push(hk)
  if (hk.cleaned) log(`cargo clean ran; ${hk.free_gb} GB free`)
  if (hk.orphan_stash) log(`stashed orphaned changes: ${hk.orphan_stash}`)
  return hk
}

async function prStatus(label) {
  return agent(`${COMMON}
Run exactly this and return its JSON output as structured data, unchanged:
  cd ${ROOT} && python3 -I ${SCRIPTS}/pr_status.py --max-attempts ${MAX_ATTEMPTS}`,
    { label, phase: 'Repair', schema: PRS, effort: 'low' })
}

// One attempt per PR that needs it. Returns false if housekeeping says stop.
async function repairPass(tag) {
  if (DRY) return true
  const st = await prStatus(`pr status (${tag})`)
  if (!st) { report.notes.push(`pr status failed (${tag})`); return true }
  for (const pr of st.needs_repair) {
    const hk = await housekeeping(false)
    if (hk.abort) return false
    const r = await agent(`${COMMON}
Mode: **repair**. Read ${SKILL} and follow its "Repair mode" section for PR #${pr.number} ("${pr.title}", head branch ${pr.head}, current state: ${pr.state}). Previous repair attempts: ${pr.attempts}, so this is attempt ${pr.attempts + 1} of ${MAX_ATTEMPTS}.
Work only in ${WT} (housekeeping just reset it to a detached origin/main). Make exactly one attempt, then return. Do not wait for CI.`,
      { label: `repair #${pr.number}`, phase: 'Repair', schema: REPAIR })
    report.repairs.push({ pr: pr.number, title: pr.title, attempt: pr.attempts + 1, ...(r || { outcome: 'needs_human', summary: 'repair agent died' }) })
  }
  return true
}

// ---------- run ----------
phase('Housekeeping')
let hk = await housekeeping(true)
if (hk.abort) {
  report.notes.push(`aborted at start: ${hk.abort_reason || hk.error}`)
  return report
}

phase('Repair')
let alive = await repairPass('start')

phase('Select')
const sel = await agent(`${COMMON}
Run exactly this and return its JSON output as structured data (candidates and skipped, unchanged):
  cd ${ROOT} && python3 -I ${SCRIPTS}/select_candidates.py --gate-version ${GATE_VERSION}`,
  { label: 'select candidates', phase: 'Select', schema: SELECT, effort: 'low' })
if (!sel) { report.notes.push('candidate selection failed'); return report }
let candidates = sel.candidates
if (A.issues && A.issues.length) {
  const want = new Set(A.issues)
  candidates = candidates.filter(c => want.has(c.number))
  const missing = A.issues.filter(n => !candidates.some(c => c.number === n))
  if (missing.length) report.notes.push(`requested but not eligible: ${missing.map(n => '#' + n).join(', ')} (see skipped)`)
}
if (A.gateLimit && candidates.length > A.gateLimit) {
  report.notes.push(`gateLimit ${A.gateLimit}: deferred ${candidates.length - A.gateLimit} candidates to a later run`)
  candidates = candidates.slice(0, A.gateLimit)
}
report.skipped = sel.skipped
log(`${candidates.length} candidates to gate, ${sel.skipped.length} skipped by pre-filter`)

phase('Gate')
// Gates are read-only (they read code in ${WT}, which nothing mutates in this
// phase), so they run in parallel. The barrier is deliberate: the fix phase picks
// the best maxFixes passes from the full set.
const gates = await parallel(candidates.map(c => () =>
  agent(`${COMMON}
Mode: **gate-only**${DRY ? ', dry-run (do NOT post any comment; return the text you would post)' : ''}. Read ${SKILL} and follow §1, §2, and "Recording a gate failure" for issue #${c.number} ("${c.title}").
Read code in ${WT} (a fresh origin/main), not in the main checkout. Ignore the "Clean start" check; the fix step handles it. Do not create strands, branches, or files.`,
    { label: `gate #${c.number}`, phase: 'Gate', schema: GATE })
    .then(g => ({ number: c.number, title: c.title, why_eligible: c.reason, ...(g || { verdict: 'fail-environment', summary: 'gate agent died', comment_posted: false }) }))))
report.gates = gates.filter(Boolean)
const passes = report.gates.filter(g => g.verdict === 'pass')
  .sort((a, b) => (a.estimated_files ?? 9) - (b.estimated_files ?? 9))  // smallest first
log(`gate: ${passes.length} pass, ${report.gates.filter(g => g.verdict === 'fail-intrinsic').length} intrinsic fail, ${report.gates.filter(g => g.verdict === 'fail-environment').length} environment fail`)

if (DRY) {
  report.notes.push('dry run: fix and repair phases skipped')
  return report
}

phase('Fix')
const toFix = passes.slice(0, MAX_FIXES)
if (passes.length > MAX_FIXES) {
  report.notes.push(`maxFixes ${MAX_FIXES}: ${passes.length - MAX_FIXES} passing issues left for a later run: ${passes.slice(MAX_FIXES).map(p => '#' + p.number).join(', ')}`)
}
for (const g of toFix) {
  if (!alive) break
  hk = await housekeeping(false)
  if (hk.abort) { alive = false; report.notes.push(`stopped before #${g.number}: ${hk.abort_reason || hk.error}`); break }
  const r = await agent(`${COMMON}
Mode: **sweep**. Read ${SKILL} and follow it for issue #${g.number} ("${g.title}"), in sweep mode: work in ${WT} (housekeeping just reset it to a detached, fresh origin/main), label the PR \`autofix\`, and stop after the PR is open and the push is confirmed. Skip §7 (no CI watching).
A gate-only pass already approved this issue: "${g.summary}". Re-check the gate quickly against what you find in the code; if it now fails, record it per "Recording a gate failure" and return status=gate_failed.
Return status=stopped (with the reason) if you hit a §5 failure you caused and can't fix within scope.`,
    { label: `fix #${g.number}`, phase: 'Fix', schema: FIX })
  report.fixes.push({ number: g.number, title: g.title, ...(r || { status: 'stopped', summary: 'fix agent died' }) })
  // Earlier PRs may have finished CI while this fix ran.
  alive = await repairPass(`after #${g.number}`)
}

phase('Repair')
if (alive) await repairPass('end')
report.finalPrs = await prStatus('pr status (final)')
return report
