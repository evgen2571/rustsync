#!/usr/bin/env bash
set -euo pipefail
if [[ -z ${EPOCHREALTIME:-} ]]; then
    echo 'The benchmark requires Bash 5 or newer for its microsecond clock.' >&2
    exit 1
fi
export RUSTSYNC_PROFILE=release
source "$(dirname -- "${BASH_SOURCE[0]}")/local-server.sh"

workspace="$run_dir/workspace"
mkdir "$workspace"
cli init "$workspace" >&2
for ((i=0; i<1000; i++)); do
    printf 'file %04d\n%01014d' "$i" 0 > "$workspace/file-$i"
done
cli sync "$workspace" --json > "$run_dir/initial.json"
jq -e '.ok and .uploaded_blobs == 1000' "$run_dir/initial.json" >/dev/null
results="$run_dir/results.jsonl"
measure() {
    local scenario=$1 expected=$2 start finish elapsed
    start=${EPOCHREALTIME/./}
    cli sync "$workspace" --json > "$run_dir/sync.json"
    finish=${EPOCHREALTIME/./}
    elapsed=$((finish - start))
    case "$expected" in
        none) jq -e '.ok and (.published | not) and .uploaded_blobs == 0 and .downloaded_blobs == 0 and .uploaded_bytes == 0 and .downloaded_bytes == 0' "$run_dir/sync.json" >/dev/null ;;
        one) jq -e '.ok and .published and .uploaded_blobs == 1 and .downloaded_blobs == 0' "$run_dir/sync.json" >/dev/null ;;
        large) jq -e '.ok and .published and .uploaded_blobs > 1 and .uploaded_bytes > 10000000 and .downloaded_blobs == 0' "$run_dir/sync.json" >/dev/null ;;
    esac
    jq -c --arg scenario "$scenario" --argjson elapsed "$elapsed" \
        '{scenario: $scenario, elapsed_ms: ($elapsed / 1000), uploaded_blobs, uploaded_bytes, downloaded_blobs, downloaded_bytes, synced_revision, published}' \
        "$run_dir/sync.json" >> "$results"
}
for ((sample=1; sample<=3; sample++)); do
    measure '1,000 small files, no changes' none
done
for ((sample=1; sample<=3; sample++)); do
    printf 'Changed file, sample %s\n' "$sample" > "$workspace/file-0"
    measure '1 changed file among 1,000' one
done
for ((sample=1; sample<=3; sample++)); do
    head -c 10000000 /dev/urandom > "$workspace/large.bin"
    measure '10 MB file, new contents' large
done
for ((sample=1; sample<=3; sample++)); do
    measure 'Second sync with unchanged data' none
done
printf '# Local sync benchmarks\n\n'
printf 'Measured on %s with `%s`, release binaries, loopback HTTP and a fresh SQLite store.\n\n' \
    "$(date -u +%Y-%m-%d)" "$(rustc --version)"
printf 'System: `%s`.\n\n' "$(uname -srm)"
printf 'Source: `%s` with local changes, if any, shown by `git status --short`.\n\n' "$(git -C "$repo_root" rev-parse --short HEAD)"
if [[ -r /proc/cpuinfo ]]; then
    printf 'CPU: %s.\n\n' "$(awk -F ': ' '/^model name/ {print $2; exit}' /proc/cpuinfo)"
fi
printf 'Each result is the median of three sequential runs. Timings include CLI process startup, scanning, encryption, HTTP and storage. Build, server startup and fixture generation are excluded. Each small file initially contains 1,024 bytes; MB means 1,000,000 bytes.\n\n'
printf '| Scenario | Median ms | Uploaded blobs | Uploaded bytes | Downloaded blobs |\n'
printf '| --- | ---: | ---: | ---: | ---: |\n'
jq -rs 'group_by(.scenario) | sort_by(.[0].scenario)[] | sort_by(.elapsed_ms) | .[1] |
    "| \(.scenario) | \(.elapsed_ms | . * 100 | round / 100) | \(.uploaded_blobs) | \(.uploaded_bytes) | \(.downloaded_blobs) |"' "$results"
printf '\nRun `scripts/benchmark.sh` to reproduce. The script checks every sample: unchanged syncs transfer zero blobs and do not publish; changing one small file uploads exactly one blob, reusing the other 999 remote files. The 10 MB cases use fresh random data and require multiple uploaded chunks. Transfer counts exclude manifests and HTTP framing. These are local measurements, not network throughput guarantees.\n'
if [[ -n ${RUSTSYNC_BENCHMARK_RESULTS:-} ]]; then
    cp -- "$results" "$RUSTSYNC_BENCHMARK_RESULTS"
fi
