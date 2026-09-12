# Development

[README](../README.md) · [Storage reference](internals.md) · [Security](security.md)

## Build and run

Use a Rust toolchain with Cargo and a native compiler toolchain. The workspace
uses edition 2024 and resolver 3. CI follows stable Rust on Linux; no minimum
Rust version is declared or pinned.

From the repository root:

```sh
cargo build --workspace --locked
cargo run --locked -p rustsync-server -- --help
cargo run --locked -p rustsync -- --help
```

`cargo build` creates `target/debug/rustsync`, its compatibility command `target/debug/rustsync-cli`, and
`target/debug/rustsync-server`. Add `--release` for optimized binaries under
`target/release/`. On Windows, executable filenames have an `.exe` suffix.

## Validation

Run the same checks used for normal development:

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

Target the relevant integration suite while working:

```sh
cargo test --locked -p rustsync --test sync_workflow
cargo test --locked -p rustsync --test device_bootstrap_e2e
cargo test --locked -p rustsync-core --test local_workspace_engine
cargo test --locked -p rustsync-server --test http
cargo test --locked -p rustsync-server --test storage
```

The sync workflow tests use real local files and encryption with a controlled
remote implementation for failures and races. The device bootstrap test runs
against a local HTTP server and exercises enrollment, two-device merging, and
deletion-conflict resolution. Storage tests cover corruption, restart repair,
root locking, and concurrent publication. HTTP tests cover signatures,
permissions, replay rejection, body limits, and opaque object transfer.

Generate crate API documentation locally:

```sh
cargo doc --workspace --no-deps --locked
```

The device bootstrap scenario uses `IndexedFsStorage`, matching the shipped
server. `multi_device_process_e2e` runs fresh CLI processes against a separate
server application process, kills and restarts that process, then verifies
merges, deletion, persisted revisions, and repeated no-op syncs. Client nonce
tests also launch eight processes and check every generated value for reuse.

## Making changes

Reproduce bugs with a focused regression test, then run the affected suite and
the workspace checks. For sync changes, verify deletions and conflict copies as
well as successful transfers. For storage changes, test interrupted publication,
reopening data, and corruption reporting.

Changing an internal helper does not automatically mean adding a CLI command.
Keep the command reference aligned with the actual parser. When changing stored
data or wire formats, update the storage reference and state whether existing
stored data must be replaced.

The repository currently has no automated binary-release workflow. Build and
install the binaries from source.
