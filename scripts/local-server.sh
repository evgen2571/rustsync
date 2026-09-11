#!/usr/bin/env bash
# Shared setup for repository examples. Source this file from Bash.
set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
for dependency in cargo curl jq; do
    command -v "$dependency" >/dev/null || { echo "Required command missing: $dependency" >&2; exit 1; }
done
profile=${RUSTSYNC_PROFILE:-debug}
case "$profile" in
    debug) build_args=() ;;
    release) build_args=(--release) ;;
    *) echo 'RUSTSYNC_PROFILE must be debug or release' >&2; exit 1 ;;
esac
cargo build --manifest-path "$repo_root/Cargo.toml" --locked -p rustsync-cli -p rustsync-server "${build_args[@]}" >&2
target_dir=$(cargo metadata --manifest-path "$repo_root/Cargo.toml" --no-deps --format-version 1 | jq -r .target_directory)
client="$target_dir/$profile/rustsync-cli"
server="$target_dir/$profile/rustsync-server"
run_dir=$(mktemp -d "${TMPDIR:-/tmp}/rustsync-example.XXXXXXXX")
server_pid=
cleanup() {
    if [[ -n "$server_pid" ]]; then
        kill "$server_pid" 2>/dev/null || true
        wait "$server_pid" 2>/dev/null || true
    fi
    if [[ ${RUSTSYNC_KEEP_DATA:-0} == 1 ]]; then
        echo "Example data retained at $run_dir" >&2
    else
        rm -rf -- "$run_dir"
    fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
port=${RUSTSYNC_EXAMPLE_PORT:-$((30000 + RANDOM % 20000))}
server_url="http://127.0.0.1:$port"
"$server" --host 127.0.0.1 --port "$port" --storage-dir "$run_dir/server" >"$run_dir/server.log" 2>&1 &
server_pid=$!
ready=0
for ((attempt=0; attempt<100; attempt++)); do
    if ! kill -0 "$server_pid" 2>/dev/null; then
        cat "$run_dir/server.log" >&2
        exit 1
    fi
    if [[ $(cat "$run_dir/server.log") == *'Server running on '* ]] &&
        curl --fail --silent --max-time 1 "$server_url/health" | jq -e '.status == "ok"' >/dev/null; then
        ready=1
        break
    fi
    sleep 0.1
done
if [[ $ready != 1 ]]; then
    echo "Server did not become ready at $server_url" >&2
    cat "$run_dir/server.log" >&2
    exit 1
fi
cli() { "$client" --server-url "$server_url" "$@"; }
