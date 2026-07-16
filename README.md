# RustSync

RustSync is a small Rust project for syncing encrypted workspace files through a
server.

The server stores encrypted objects. Plaintext files and encryption keys stay on
the local devices.

## Quickstart

Build and start the server in one terminal:

```sh
cargo run -p rustsync-server
```

The default server URL is `http://127.0.0.1:3000`.

In another terminal, create the first workspace:

```sh
cargo run -p rustsync-cli -- init ./owner
```

The command prints a workspace ID. Save it for the second-device setup.

Add or edit files in `./owner`, then sync them:

```sh
cargo run -p rustsync-cli -- sync ./owner
```

Check local and remote state with:

```sh
cargo run -p rustsync-cli -- status ./owner
cargo run -p rustsync-cli -- remote-status ./owner
```

## Add a second device

Create a pending identity and request access to the workspace. Replace
`<workspace-id>` with the ID printed by `init`:

```sh
cargo run -p rustsync-cli -- device request <workspace-id> ./device
```

On the owner device, list pending requests:

```sh
cargo run -p rustsync-cli -- device list-requests ./owner
```

Approve the request using its printed request ID:

```sh
cargo run -p rustsync-cli -- device approve <join-request-id> ./owner
```

Back on the second device, download the workspace key and finish setup:

```sh
cargo run -p rustsync-cli -- device bootstrap <workspace-id> ./device
cargo run -p rustsync-cli -- sync ./device
```

Now both directories can sync through the same server.

## Conflicts

RustSync tries to merge non-overlapping text changes. Other conflicts are kept
as a separate remote copy.

List unresolved conflicts:

```sh
cargo run -p rustsync-cli -- conflicts ./owner
```

Keep one side, then sync again:

```sh
cargo run -p rustsync-cli -- resolve path/to/file.txt --keep-local ./owner
cargo run -p rustsync-cli -- sync ./owner
```

Use `--keep-remote` instead to keep the remote copy.

Preview a reconciliation without changing local or remote state:

```sh
cargo run -p rustsync-cli -- sync --dry-run ./owner
```

## Server URL

Use `--server-url` when the server is not running on the default address:

```sh
cargo run -p rustsync-cli -- \
  --server-url http://127.0.0.1:4000 \
  sync ./owner
```

The server currently uses these defaults:

- host: `127.0.0.1`
- port: `3000`
- storage: `./server-storage`

Override them directly:

```sh
cargo run -p rustsync-server -- \
  --host 0.0.0.0 \
  --port 4000 \
  --storage-dir ./data
```

The same settings can be provided through `RUSTSYNC_HOST`, `RUSTSYNC_PORT`,
and `RUSTSYNC_STORAGE_DIR`.

## Workspace layout

```text
crates/
  rustsync-protocol/   Shared IDs, DTOs, auth, and API types
  rustsync-core/       Local workspace state, manifests, and encryption
  rustsync-client/     Signed HTTP client
  rustsync-cli/        User-facing command-line application
  rustsync-server/     HTTP server and encrypted object storage
```

## Development

Format-check the workspace:

```sh
cargo fmt --all -- --check
```

Run all tests:

```sh
cargo test --workspace
```

Run Clippy with warnings treated as errors:

```sh
cargo clippy --workspace --all-targets -- -D warnings
```

Storage details and development limitations are documented in
[docs/storage.md](docs/storage.md).
