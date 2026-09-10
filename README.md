# RustSync

Sync a directory between your devices through a server you run. Files and
manifests are encrypted on each device before upload. The server stores the
encrypted objects and enforces device permissions.

Run `sync` when you want to exchange changes. RustSync propagates edits and
deletions, merges compatible text edits, and keeps conflicting versions for
you to resolve. Unchanged files keep their local modification times and reuse
their remote blobs.

RustSync is pre-release software. Each encrypted object is limited to 1 MiB,
persisted formats can change, and whole-sync power-loss recovery is not
guaranteed. Keep an independent backup.

## Try it locally

You need stable Rust, Cargo, and a native compiler toolchain. From the repository
root, start the server in one terminal:

```sh
cargo run --locked -p rustsync-server
```

In another terminal, publish two versions of a file from a fresh directory:

```sh
demo_dir="$(mktemp -d)"
cargo run --locked -p rustsync-cli -- init "$demo_dir"
printf 'Hello from RustSync\n' > "$demo_dir/hello.txt"
cargo run --locked -p rustsync-cli -- sync "$demo_dir"
printf 'An updated greeting\n' > "$demo_dir/hello.txt"
cargo run --locked -p rustsync-cli -- sync "$demo_dir"
cargo run --locked -p rustsync-cli -- remote-status "$demo_dir"
cargo run --locked -p rustsync-cli -- sync "$demo_dir"
```

Remote status reports revision 2. The last sync makes no new publication.
The server listens at `http://127.0.0.1:3000` and keeps data in `./server-storage`.
Stop it with Ctrl+C; restarting with the same storage directory retains the
workspace. [Enroll another device](docs/usage.md#manage-devices) to exchange files
between two directories or machines.

## Install the commands

```sh
cargo install --locked --path crates/rustsync-cli
cargo install --locked --path crates/rustsync-server
```

The executables are `rustsync-cli` and `rustsync-server`. Some application
messages use `rustsync`; substitute `rustsync-cli`. Use `--server-url URL` on
client commands when your server uses another address.

## Documentation

- [Usage and recovery](docs/usage.md)
- [Command reference](docs/cli.md)
- [Server configuration and backups](docs/server.md)
- [Security and limitations](docs/security.md)
- [Storage formats](docs/storage.md)
- [Development and checks](docs/development.md)
