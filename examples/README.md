# Examples

From the repository root, start a persistent local server:

```sh
docker compose -f examples/docker-compose.yml up -d
```

Or run the complete two-device demo, which starts its own temporary server:

```sh
examples/demo.sh
```

The demo requires Bash, Cargo and a Rust compiler, curl, jq, and the native
compiler toolchain needed by the crates. It builds the binaries, creates two
fresh devices, approves enrollment, exchanges edits, creates a conflict, and
resolves it. It checks the actual file contents and conflict state, then stops
its server and removes its temporary directory.

Set `RUSTSYNC_KEEP_DATA=1` to retain the example directories for inspection.
The path is printed at exit. The server still stops; to inspect remote status,
restart `target/debug/rustsync-server --storage-dir <printed-path>/server`
and use the matching client `--server-url`.

The example server binds to a random localhost port. Set
`RUSTSYNC_EXAMPLE_PORT=31000` to choose a port. An occupied port makes startup
fail without touching the other server. Set `RUSTSYNC_PROFILE=release` to run
the demo with optimized binaries.

Copy `rustsyncignore.example` to `.rustsyncignore` in your workspace before
its first sync. It excludes build output, local credentials and temporary files.
Already tracked files remain tracked even if a new ignore rule matches them.

For repeatable transfer measurements, run `scripts/benchmark.sh`. It needs
Bash 5 or newer, always builds release binaries, and prints a Markdown table.
Set `RUSTSYNC_BENCHMARK_RESULTS=/tmp/rustsync-samples.jsonl` to save all samples.
The demo and benchmark use disposable local identities; automatic approval in
these scripts does not replace fingerprint verification for real devices.
