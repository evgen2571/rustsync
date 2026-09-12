# Repository scripts

Run these commands from the repository root. Both scripts build the required
binaries, start a temporary loopback server, and clean up their process and data
on exit. They require Bash, Cargo, a Rust and native compiler toolchain, curl,
and jq. The benchmark requires Bash 5 or newer.

| Command | Purpose |
| --- | --- |
| `scripts/demo.sh` | Enroll two devices, exchange edits, preserve and resolve a conflict, then verify contents and diagnostics |
| `scripts/benchmark.sh` | Measure release syncs and assert blob reuse, no-op transfers, and large-file chunking |

`local-server.sh` is shared setup sourced by these scripts, not a standalone
server command. Use [deployment](../docs/deployment.md) for a persistent service.

| Variable | Effect |
| --- | --- |
| `RUSTSYNC_KEEP_DATA=1` | Retain temporary data and print its path; the server still stops |
| `RUSTSYNC_EXAMPLE_PORT=31000` | Choose a localhost port instead of a random port between 30000 and 49999 |
| `RUSTSYNC_PROFILE=release` | Use optimized demo binaries; the benchmark always uses release |
| `RUSTSYNC_BENCHMARK_RESULTS=/tmp/samples.jsonl` | Save every benchmark sample outside the temporary directory |

An occupied port fails startup. To inspect retained data, restart
`target/debug/rustsync-server --port PORT --storage-dir RETAINED_PATH/server`
with the original port and use the matching client `--server-url`. For a release
run, use `target/release/` instead. A custom Cargo target directory changes these
binary paths.

Automatic approval is appropriate for the disposable identities created here.
For real devices, compare fingerprints through a trusted channel before approval.
See [benchmark methodology and results](../docs/benchmarks.md).
