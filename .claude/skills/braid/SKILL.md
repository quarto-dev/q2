---
name: braid
description: Reference for braid, this project's issue tracker (the "skein" of "strands"). Use whenever you need braid command syntax — finding ready work, creating/updating/closing strands, dependencies, ready/blocked queries — or when a user mentions braid, a strand, or a bd- issue id and you need the authoritative, version-matched usage. The body defers to `braid agents-info` for the full guide.
---

<!-- the lines below are managed by `braid agents-info --install`; the
     frontmatter above is preserved across re-installs (it lives outside
     the BEGIN/END markers). -->
<!-- BEGIN BRAID (managed by `braid agents-info --install`) -->
# braid issue tracking

This project tracks issues ("strands") with **braid**. For the
authoritative, version-matched usage guide — every command, flag, and
convention — run:

    braid agents-info

Core loop: `braid ready` finds workable strands; claim one with `braid
update <id> --status in_progress --assignee <you>`; leave a trail with
`braid comment <id> "..."`; finish with `braid close <id> --reason "..."`.
File discovered work as you go in one shot:

    braid create "<title>" --type <task|bug|...> --deps discovered-from:<current-id>

Attribute your changes with `BRAID_AUTHOR=<you>`.
<!-- END BRAID -->
