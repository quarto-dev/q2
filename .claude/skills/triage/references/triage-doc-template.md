# Triage doc template

Copy the body below into `claude-notes/issue-reports/<N>/triage.md` and fill it in.

```markdown
# Issue #<N> — <one-line headline>

- **GitHub**: https://github.com/quarto-dev/q2/issues/<N>
- **Reporter**: @<login> (<name>), <date>
- **Triage date**: <today>
- **Worktree**: `.worktrees/issue-<N>` (branch `issue-<N>`, based on `main` @ `<short-sha>`)
- **Beads issue**: bd-XXXX (or "none — see Outcome")
- **Scope**: which part(s) of the issue this triage covers, and which it explicitly excludes.

## Summary

One paragraph: what the user reported, whether you reproduced it, and the conclusion in one sentence ("real bug, fix is small", "working as intended, see explanation below", "duplicate of bd-XXXX", etc.).

## Reproduction

Exact commands. Show input and observed-vs-expected output. Reference the fixture under `claude-notes/issue-reports/<N>/`.

## Localization (if applicable)

`file:line` pointers to the code involved. If the bug is "X is missing", note where the analogous working code lives so the fix has a model to copy.

## Open questions — resolved during triage

For each open question raised during investigation, write the question, the experiment that answered it, and the conclusion. **Do not leave forwarded TODOs.** If a question can't be answered in this triage, escalate it to the user before declaring the triage done.

## Outcome / recommended next step

One of:
- "Filed bd-XXXX with fix scope below."
- "Working as intended; see explanation. Will respond on GH."
- "Duplicate of bd-XXXX."
- "Documentation gap; will update `docs/...`."
- "Need more info from reporter; will respond on GH with these questions: ..."

## Verification commands used

The exact `gh`, `cargo`, etc. commands you ran, so a future reader can re-do the investigation.

## Cross-references

- bd-XXXX entries
- related claude-notes/ documents
- relevant `CLAUDE.md` rules
```
