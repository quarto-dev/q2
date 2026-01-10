Quarto Hub is a collaborative editor for Quarto projects.
Do you want to know more? Read on.

## DO NOT USE FOR PRIVATE DATA YET

We are planning on ways for users to collaborate using a secure server, but that is not currently implemented. Currently, Quarto Hub defaults to automerge's public sync servers (see the `automerge` section below.).

In addition, the current version of the collaboration mechanism is such that:

- there's no read-only permission: if you give someone the project id, then they have the ability to edit your documents.
- there's no deletion of past versions: automerge is append-only. If you write something in the document and later delete it, that previous version exists as long as someone holds a copy of the project.

## Compatibility

We aim for the functionality in this to be fully compatible with what's available in the [current version of Quarto](https://quarto.org).

### QMD

The main user-facing change is that we are using a particular dialect of Markdown we have designed especially for Quarto: [Quarto Markdown](https://github.com/quarto-dev/quarto-markdown).

We are working on tooling for conversion of projects to the new syntax, and expect this to not be a barrier for migration.

## Under the hood

### Collaborative editing

We're using [automerge](https://automerge.org) for the collaboration infrastructure.
The `automerge` libraries offer a CRDT for JSON data.
Under the hood, Quarto projects in Quarto Hub are a big collection of interlinked JSON documents with the appropriate format.

We plan for the automerge schema of Quarto projects to be open source, and we'll make that available for users directly and as TypeScript libraries.
This way, developers can work on other applications that can read and edit Quarto projects collaboratively, independent of the Quarto Hub implementation itself.

#### Security considerations

Currently, Quarto Hub's projects sync to `wss://sync.automerge.org` by default. You can run your own sync servers as long as they implement the web socket protocol in the same way as [Ink and Switch's TypeScript implementation works](https://github.com/automerge/automerge-repo-sync-server). If you're comfortable with e.g. Tailscale, you could likely set up a local VPN and private sync servers today. We don't plan on ever removing your ability to do this and, in this case, if you host the quarto-hub static HTML app and servers yourself, no one in the Quarto project (or Posit) would in principle be able to see your data.

If you're concerned about private data and security though, we ~~beg~~ encourage you to do a thorough review of all relevant code bases and technologies before using this with sensitive data! And, as a general reminder, this is open source software, and we offer no warranty or guarantees for fitness of purpose (in secure settings or otherwise).

### Quarto in WASM 

`quarto-hub` uses an in-progress port of Quarto to Rust designed from the ground up to be compilable to WASM, including the `qmd`->`html` rendering pipeline, and the code paths between the native Rust version and the WASM version are shared.

This design ensures that as future versions Quarto evolves, it will be easy for Quarto Hub to pick up the same functionality.