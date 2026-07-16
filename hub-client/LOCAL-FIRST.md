# Local-first documents & connecting to a hub

Quarto Hub opens straight into a usable editor — **no sign-in required**. Your
projects are real Automerge documents that live in your browser's local
storage. Signing in to a hub is optional, and only needed when you want to
sync a project across devices or collaborate with other people.

> This is about the **document / account model** you see as a user. For how the
> app itself keeps working with no network (asset/service-worker caching), see
> [`OFFLINE.md`](./OFFLINE.md).

## Working locally (no account)

When you open Quarto Hub for the first time:

- The **project selector** appears immediately — there is no login gate.
- **Create New Project** and **Import from ZIP** create a project that lives
  entirely in your browser. No sync server is contacted.
- Everything you edit is saved to local storage as you go, so your work
  survives a reload or closing the tab. Your edits are attributed to a stable
  local author ("You") that stays consistent across reloads.

A local project is private to this browser. It is not uploaded anywhere and is
not visible on any other device until you publish it to a hub.

## Connecting to a hub

Use the **Connect to a hub** control in the header (top-right) when you want to
sync or collaborate. It:

1. Signs you in (if you don't already have a session), then
2. Lets you open or create projects hosted on the hub server.

Once you're signed in, the header shows **"Signed in as _you_ · Sign out"**.
New projects you create while connected to a hub are hosted on that hub and can
be shared; projects you create while working locally stay local.

Signing out (or a session expiring) never disturbs a local project — only
hub-backed projects need a valid session, and losing one simply returns you to
the project list.

## Working offline with a hub project

A hub project you've opened before is **cached in this browser**, so you can
keep working on it even with no connection — for example on a plane, or after
signing out:

- Opening a cached hub project while offline (or logged out) loads it from the
  local cache and is **fully editable**. You don't need to sign in first.
- Your offline edits are attributed to the local author ("You") while you're
  disconnected.
- When you reconnect (come back online / sign in), your offline edits **sync up
  to the hub automatically**, and the whole editing history — offline and
  online — shows as one author.

A hub project you've **never** opened on this device can't be opened offline
(there's nothing cached yet); the app tells you to reconnect and sign in.

## What requires a hub

| Action | Local (no account) | Connected to a hub |
| --- | --- | --- |
| Create / edit / preview a project | ✅ | ✅ |
| Persist across reloads (same browser) | ✅ | ✅ |
| Open & edit a *previously-opened* hub project offline | — | ✅ (from cache) |
| Sync across your own devices | — | ✅ |
| Real-time collaboration with others | — | ✅ |
| Share a project link | — | ✅ |

## Notes

- **Connect to a hub** (header, account-level) is different from **Connect to
  Project** (in the actions row), which joins an *existing* hub project by its
  document id and sync-server URL.
- Publishing an existing **local** project up to a hub (keeping its history) is
  a separate, planned feature — for now, local projects stay local and hub
  projects are created directly on the hub.
