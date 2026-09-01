# Paired-package playbook (react / react-dom)

Some packages must move in lockstep (`react` + `react-dom` is the known case).
Snyk files **one PR per package**, so the pair arrives as two PRs that
conflict with each other: whichever merges first makes the other unmergeable.

Precedent: PR #511 (react 19.2.7→19.2.8) merged clean; PR #512 (react-dom)
then conflicted and was fixed by merging main into the branch and resolving
the package-file conflicts by **taking the new version of both packages**
(commit `91977474`).

## Procedure

1. Identify the sibling PR (`gh pr list --author posit-snyk-bot --search <pkg>`).
   Decide an order with the user if both are open; remediate one at a time.
2. First PR: usually only needs the generic workflow (merge main, root
   `npm install`, verify).
3. Second PR: merge `origin/main` (which now contains the first upgrade).
   In conflicted `package.json` / lockfiles, keep the new version of **both**
   packages — never let one of the pair sit on the old version.
4. Regenerate lockfiles from the repo root (`npm install`), verify the tree is
   clean after a fresh install, then run the standard verification battery.

## Watch for

- Version references outside package files (step 4 of the main workflow):
  react versions can appear in ts-packages' own package files (the #512
  conflict was in pandoc-diff's package files).
- If the two PRs upgrade to *different* versions (bot lag), align both to the
  newer one and note it in the commit message.
