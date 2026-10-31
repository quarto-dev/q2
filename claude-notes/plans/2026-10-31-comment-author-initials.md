---
title: 'Comment rows: author initials next to the color dot (GH #977)'
date: 2026-10-31
description: 'Derives author initials from the attribution identity name and shows them beside the color dot on expanded comment rows.'
---

**Strand:** bd-4asq3ei5
**Issue:** https://github.com/quarto-dev/q2/issues/977 (comments surface only;
the text attribution view and the Authors list stay open on the issue)

## Overview

Expanded comment rows in `CommentBlock.tsx` identify the author with only a
color dot, so the reader has to match colors against the Authors list.

Rows now show initials beside the dot. They are derived from the `name` that
`astContext.attributionActors` already carries, which in the hub comes from the
index document's `identities` map ("Charlie Gao" gives "CG"). No schema or wire
change is needed.

Actors with no profile get a fallback name of `key.slice(0, 8)` in
`buildIdentityMap` (`hub-client/src/hooks/useAttribution.ts`). That name is not
a person's name, so those rows keep the dot alone.

## Work items

- [x] Failing tests for `authorInitials` and for the comment row
- [x] `authorInitials` helper in `framework/attribution.tsx`
- [x] Render initials in the expanded comment row
- [x] vitest, `tsc --noEmit`, ESLint
- [x] PR (`Part of #977`)
