# RustSync

RustSync is a Rust pet-project for syncing encrypted workspace data through a server.

The main idea is simple:

- the local machine owns secrets and plaintext
- the server stores encrypted objects and workspace metadata
- the protocol crate defines the wire language shared by every side
- the CLI is the user-facing entry point

## Workspace layout

```text
crates/
  rustsync-protocol/   Shared IDs, DTOs, auth payloads, route vocabulary, API errors
  rustsync-core/       Local workspace engine, manifests, encryption, device identity
  rustsync-client/     Signed HTTP client for talking to a RustSync server
  rustsync-cli/        Command-line app built on core + client
  rustsync-server/     HTTP server, auth middleware, replay checks, object storage
```
## Running checks

Format:

```sh
cargo fmt --check
```

Run all tests:

```sh
cargo test --workspace
```
