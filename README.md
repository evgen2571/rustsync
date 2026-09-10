# RustSync

RustSync synchronizes a directory between devices through a server you run.
Devices encrypt file contents and manifests before uploading them. The server
stores encrypted objects and controls which devices can access a workspace.

Synchronization is manual. Run `sync` to exchange changes, merge supported text
edits, and preserve conflicting versions for review.

RustSync is pre-release software. Persisted formats can change, each encrypted
object is limited to **1 MiB**, and power-loss durability is not guaranteed.
Keep an independent backup of files you need to retain.

## Current blockers

The shipped SQLite server currently fails to publish changes after revision 1.
Rapid separate CLI processes can also collide on request nonces. Initial setup
and download work, but repeated editing needs a code fix before normal use.
See [known issues and reproductions](docs/known-issues.md). The guides describe
the command behavior and identify where these bugs interrupt it.

## Get started

You need Rust and Cargo, plus a native compiler toolchain for dependencies.
The workspace uses Rust edition 2024; CI checks the stable toolchain on Linux.
There is no declared minimum supported Rust version.

For now, leave at least one second between network commands from the same
device. Rapid separate CLI processes can reuse an authentication nonce and fail
with `ReplayDetected`. The pauses below work around this known issue.

From the repository root, start the server in one terminal:

```sh
cargo run --locked -p rustsync-server
```

In another terminal, initialize a workspace and upload a file:

```sh
cargo run --locked -p rustsync-cli -- init ./owner
printf 'Hello from RustSync\n' > ./owner/hello.txt
sleep 1
cargo run --locked -p rustsync-cli -- sync ./owner
```

Save the workspace ID printed by `init`. The server must be running for both
initialization and synchronization. These examples use
`http://127.0.0.1:3000`, with server data in `./server-storage`.

Inspect the workspace or preview another sync:

```sh
cargo run --locked -p rustsync-cli -- status ./owner
cargo run --locked -p rustsync-cli -- sync --dry-run ./owner
```

`sync` also propagates deletions. Its normal mode reconciles local and remote
changes; it does not simply replace one directory with the other.

## Install the commands

To run outside the source checkout:

```sh
cargo install --locked --path crates/rustsync-cli
cargo install --locked --path crates/rustsync-server
```

Put Cargo's installation `bin` directory on your `PATH`. The executables are
**`rustsync-cli`** and **`rustsync-server`**. Some application messages use the
shorter name `rustsync`; substitute `rustsync-cli` when following them.

The guides below use the installed commands. From a checkout, replace
`rustsync-cli` with `cargo run --locked -p rustsync-cli --`.

## Add another device

Install the client on the second device. Both devices must use the same server
URL. For a server on another machine, add `--server-url URL` to every client
command that contacts it; the URL is not saved in workspace configuration.

On the second device, request access in a fresh directory. Replace the example
IDs with those printed by the commands:

```sh
rustsync-cli device request <workspace-id> ./second --device-name laptop
```

On the owner device, inspect the request and approve it:

```sh
rustsync-cli device list-requests ./owner
sleep 1
rustsync-cli device approve <join-request-id> ./owner
```

Compare the requesting device's fingerprint with the owner's request listing
before approval. Approval grants access and uploads an encrypted workspace key
for that device.

On the second device, finish setup and download the files:

```sh
rustsync-cli device bootstrap <workspace-id> ./second
sleep 1
rustsync-cli sync ./second
```

Use `device request` and `device bootstrap` to join an existing workspace.
`init` creates a new workspace with a different identity and key.

## Documentation

| Guide | Contents |
| --- | --- |
| [Known issues](docs/known-issues.md) | Reproduced release blockers and temporary workarounds |
| [Using RustSync](docs/usage.md) | Sync behavior, conflicts, device roles, and recovery |
| [Command reference](docs/cli.md) | Every client command, argument, and option |
| [Running the server](docs/server.md) | Configuration, network access, limits, and backups |
| [Security and limitations](docs/security.md) | Encryption, local secrets, trust, and unsupported behavior |
| [Storage reference](docs/storage.md) | Local metadata, server layout, object format, and durability |
| [Development](docs/development.md) | Crate responsibilities, sync flow, tests, and contribution workflow |

## Development checks

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```
