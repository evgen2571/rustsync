# Using RustSync

[README](../README.md) · [Command reference](cli.md)

## Create a workspace

Install the binaries using the [quick start](../README.md#quick-start), then start
a server using the [deployment guide](deployment.md).

```sh
mkdir ./notes
rustsync init ./notes
printf 'Hello from RustSync\n' > ./notes/hello.txt
rustsync sync ./notes
```

For a remote server, pass `--server-url https://sync.example.com/` to `init`.
RustSync saves the address in `.rustsync/workspace.toml`; later commands use it
automatically. An explicit `--server-url` overrides the saved address for that
invocation. Older workspaces without a saved address use `http://127.0.0.1:3000`.
To change the saved address, edit the `server_url` field in that file.

## Add another device

These examples use the installed commands and an existing owner workspace at
`./notes`. Export an invite on the owner device:

```sh
rustsync invite ./notes --output notes.invite
```

Send `notes.invite` to the new device through a trusted channel. It contains the
existing workspace ID and server URL, with no private keys or access grant.
Use an address reachable from both devices. If the owner uses localhost through
a tunnel, export with `--server-url https://sync.example.com/` to include the
address the new device should use. Invites do not expire and are not signed;
verify their origin and server address before joining.

On the new device, request access from a fresh directory:

```sh
mkdir ./notes-laptop
rustsync join notes.invite ./notes-laptop --device-name laptop
```

Compare the request's device fingerprint with the owner through a trusted
channel. On the owner device, inspect and approve the request:

```sh
rustsync device list-requests ./notes
rustsync device approve <join-request-id> ./notes
```

On the new device, retrieve the workspace key and files:

```sh
rustsync join notes.invite ./notes-laptop --finish
rustsync sync ./notes-laptop
```

Keep the invite until setup finishes. Finishing before approval fails without
discarding the pending identity; rerun after approval and key delivery. Setup
saves the server address for subsequent commands. The generated workspace ID
remains the sole workspace identity. The low-level `device request` and
`device bootstrap` commands remain available for scripts that already know it.

## Everyday synchronization

A workspace is a directory with a `.rustsync` metadata directory at its root.
Each enrolled device has its own copy of the files, identity, and workspace key.
The server holds a shared current snapshot.

Edit files normally, then run:

```sh
rustsync sync ./notes
```

Run the same command on another enrolled device to receive the changes.
There is no background watcher. Files and deletions move between devices only
when you invoke sync.

During transfers, stderr reports each uploaded or downloaded blob. Large files
use several blobs. The final stdout summary shows the resulting revision,
uploaded and downloaded bytes, blob counts, and unresolved conflicts:

```text
Synced revision 12
Uploaded: 34816 bytes (2 blobs)
Downloaded: 8192 bytes (1 blob)
Conflicts: 0
```

Byte totals count encrypted file data, including chunk framing, but exclude
manifests, HTTP headers, and other API requests. Reused blobs add no transfer
bytes. Retries count bytes sent again. Dry runs show the plan without a transfer
summary. A no-op sync reports zero file-data bytes in both directions.

For each path, RustSync compares the current local contents, the remote snapshot,
and the last synchronized base:

| Change since the base | Result |
| --- | --- |
| Neither side changed, or both sides now match | No content change |
| Only local changed | Publish the local change, including deletion |
| Only remote changed | Apply the remote change, including deletion |
| Both changed a file differently | Attempt a text merge; otherwise preserve a conflict |
| A directory was replaced or deleted while the other side changed its contents | Preserve the conflicting trees for resolution |

A sync that only receives remote changes does not publish another revision.
When publication races with another device, the client fetches the new head and
replans once. A second race returns an error; rerun sync when the competing
writer has finished.

Use one sync or resolution command at a time per local workspace. Avoid editing
files during synchronization. The client checks for edits made during network
transfers and stops when it detects them, but this is not a filesystem snapshot
or a lock against other programs.

## Ignore development files

Create `.rustsyncignore` at the workspace root before the first sync:

```gitignore
# Build output and dependencies
/target/
node_modules/
*.log
!keep.log
**/cache/
.env
```

Patterns use Gitignore syntax, including comments, `*`, `?`, `**`, character
classes, leading `/` for root-relative paths, trailing `/` for directories,
and `!` to include a matching path again. An excluded directory must itself be
included again before its children can be included. Only the root
`.rustsyncignore` is read; `.gitignore` and global Git settings are not used.

Rules exclude new local paths from scanning and staging. Already tracked paths
remain synchronized, even if a later rule matches them. Add exclusions before
syncing secrets or generated files. The ignore file itself is synchronized
unless excluded. Remote tracked files can still arrive on a new device.

Ignored local files survive pulls, including `--discard-local`. If a remote
change would overwrite an ignored local path, the pull fails before deleting
files. Move the local path or adjust the rules before retrying. `.rustsync`
metadata and `.rustsync-tmp-*` transfer temporary files are always excluded.

## Preview and inspect

```sh
rustsync status ./notes
rustsync remote-status ./notes
rustsync sync --dry-run ./notes
rustsync doctor ./notes
```

`status` reports local information and queries the server for its current
revision and device role. If that query fails, it keeps the local report but
marks the server unavailable and returns a failure exit status.
`remote-status`, dry runs, and `doctor`
require a reachable server and an enrolled identity.

Dry runs report `unchanged`, `upload`, `download`, `merge`, or `conflict` actions.
They leave local files and sync metadata unchanged. File-content merge decisions
are deferred to an actual sync.

`doctor` checks connectivity, authentication, registered workspace key material,
and the content hashes of cached objects. It also reports the pending sync phase
and unresolved-conflict count. It does not repair storage or prove backup integrity.

## Resolve conflicts

Automatic merging supports UTF-8 text where each version has the same number of
lines as the base and changes do not overlap. Identical edits to the same line
also merge. Binary content, inserted or removed lines, and incompatible edits
fall back to conflict preservation.

The local side stays at the original path. The remote side, when present, is
stored beside it under a name such as:

```text
notes.txt
notes.txt.rustsync-conflict-remote-r7
```

A numeric suffix is added if that copy name is already reserved. Directory
conflicts preserve the remote subtree beneath the copy directory. The conflict
copies are ordinary workspace contents and can be synchronized to other devices;
the unresolved-conflict registry itself is local metadata.

List and inspect the recorded paths:

```sh
rustsync conflicts ./notes
```

Choose a version:

```sh
rustsync resolve notes.txt --keep-local ./notes
rustsync sync ./notes
```

Or keep the remote side:

```sh
rustsync resolve notes.txt --keep-remote ./notes
rustsync sync ./notes
```

If you want a manual merge, edit the original local file to contain the desired
result, then resolve with `--keep-local` and sync.

For a deletion conflict, the deleted side has no file to inspect. Choosing that
side removes the path, or leaves it absent. The conflict listing can still name
a remote-copy path even when the remote side is a deletion and no copy exists.

## Replace local contents deliberately

To discard local changes and use the remote snapshot:

```sh
rustsync sync --discard-local --yes ./notes
```

Back up anything you want to keep first. This mode removes local-only files,
applies remote contents, clears unresolved conflicts, and updates local sync
state without publishing. An empty remote workspace means an empty local working
tree apart from metadata and ignored local files.

## Manage devices

Follow the [second-device walkthrough](#add-another-device) for
request, approval, bootstrap, and initial sync. Use a fresh directory on the
joining device and keep its pending identity until bootstrap succeeds.

Check fingerprints through a channel you trust before approving a request.
Device names are labels, not proof of identity.

The default `member` role can synchronize and list devices. Choose `owner` only
for a device that should approve others and deliver workspace keys. To inspect
current membership:

```sh
rustsync device list ./notes
```

An owner can promote or demote an active device, or revoke its access:

```sh
rustsync device set-role <device-id> owner ./notes
rustsync device set-role <device-id> member ./notes
rustsync device remove <device-id> ./notes
```

Use the device ID from `device list`. Promote another owner before removing or
demoting the last owner. Removal blocks subsequent server requests, including
after a restart; it does not erase files or keys already held by that device.

## Recover from errors

| Symptom | Next step |
| --- | --- |
| Connection refused or timeout | Check that the server is running and that the saved `server_url` or explicit override points to it. The client timeout is 30 seconds per request. |
| `init` failed while contacting the server | Read its cleanup message. Network failures attempt to remove newly created local metadata; other failures can leave it behind. Preserve existing files before changing metadata or retrying setup. |
| Bootstrap says the identity is missing | Run `device request` in that same pending directory, then obtain approval before bootstrapping. |
| Device is not active or lacks permission | Check `device list` from an owner and confirm the intended request was approved. Members cannot approve devices. |
| Approval succeeded but key-envelope delivery failed | Approval and key delivery are separate requests. Preserve the pending identity and the error details. The CLI has no dedicated redelivery command; retrying approval may report that the request is no longer pending. |
| Authentication timestamp rejected | Check the device and server clocks. Signed requests permit a five-minute timestamp difference. |
| Workspace changed during synchronization | Finish editing, then rerun `sync`; inspect any conflicts it reports. |
| Remote head changed twice during publication | Rerun sync after other writers finish. Do not use discard-local merely to bypass a race. |
| A pending phase remains after interruption | Preserve `.rustsync`, run `doctor`, and retry normal sync. Pending state is a checkpoint, not a guarantee that every interrupted filesystem write can be rolled back. |
| Unsupported workspace entry | Remove or relocate symlinks and special files from the directory being synced. Only regular files and directories are supported. |
| Object too large | Files are chunked automatically. The encrypted manifest must still fit within 1 MiB; reduce the number or path lengths of files, or divide them into separate workspaces. There is no CLI limit override. |
| Missing or corrupt local cache or server storage | Preserve a copy of the affected data before attempting recovery. Restore a known-good backup; the client has no general repair command. |

Do not delete `.rustsync` as a routine repair step. It contains the device's
private identity, workspace keys, cached file versions, and reconciliation state.
See [backup and restore](deployment.md#backup-and-restore).
