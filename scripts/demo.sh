#!/usr/bin/env bash
set -euo pipefail
source "$(dirname -- "${BASH_SOURCE[0]}")/local-server.sh"
# Exercise saved server selection after initialization.
cli() { "$client" "$@"; }

device_a="$run_dir/device-a"
device_b="$run_dir/device-b"
mkdir "$device_a" "$device_b"
echo "Server: $server_url"
echo 'Create Device A and publish a note'
initialized=$(cli --server-url "$server_url" init "$device_a")
printf '%s\n' "$initialized"
cli invite "$device_a" --output "$run_dir/notes.invite"
printf 'Shared note\n' > "$device_a/notes.md"
cli sync "$device_a"

echo 'Request and approve Device B'
request=$(cli join "$run_dir/notes.invite" "$device_b" --device-name device-b)
printf '%s\n' "$request"
request_id=$(sed -n 's/^join request id: //p' <<<"$request")
[[ -n "$request_id" ]]
# Both devices belong to this disposable local demo, so approval is automatic.
cli device approve "$request_id" "$device_a"
cli join "$run_dir/notes.invite" "$device_b" --finish
cli sync "$device_b"
cmp "$device_a/notes.md" "$device_b/notes.md"

echo 'Edit on Device A and synchronize Device B'
printf 'Edited on Device A\n' > "$device_a/notes.md"
cli sync "$device_a"
cli sync "$device_b"
cmp "$device_a/notes.md" "$device_b/notes.md"

echo 'Make competing edits to the same line'
printf 'Chosen version from Device A\n' > "$device_a/notes.md"
printf 'Competing version from Device B\n' > "$device_b/notes.md"
cli sync "$device_a"
cli sync "$device_b"
conflicts=$(cli conflicts "$device_b")
printf '%s\n' "$conflicts"
[[ "$conflicts" == *'notes.md: local '* ]]
[[ $(cat "$device_b/notes.md") == 'Competing version from Device B' ]]

echo 'Resolve on Device B by keeping the remote version from Device A'
cli resolve notes.md --keep-remote "$device_b"
cli sync "$device_b"
cli sync "$device_a"
cmp "$device_a/notes.md" "$device_b/notes.md"
[[ $(cat "$device_a/notes.md") == 'Chosen version from Device A' ]]
for device in "$device_a" "$device_b"; do
    [[ $(cli conflicts "$device") == 'no unresolved conflicts' ]]
    cli doctor "$device"
done
echo 'Demo passed: both devices have the chosen contents and no unresolved conflicts.'
