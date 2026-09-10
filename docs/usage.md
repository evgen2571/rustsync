# Using RustSync

[README](../README.md) · [Command reference](cli.md)

## Everyday synchronization

A workspace is a directory with a `.rustsync` metadata directory at its root.
Each enrolled device has its own copy of the files, identity, and workspace key.
The server holds a shared current snapshot.

Edit files normally, then run:

```sh
rustsync-cli sync ./notes
```

Run the same command on another enrolled device to receive the changes.
There is no background watcher. Files and deletions move between devices only
when you invoke sync.

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

## Preview and inspect

```sh
rustsync-cli status ./notes
rustsync-cli remote-status ./notes
rustsync-cli sync --dry-run ./notes
rustsync-cli doctor ./notes
```

`status` is local and can work offline. `remote-status`, dry runs, and `doctor`
require a reachable server and an enrolled identity.

Dry runs report `unchanged`, `upload`, `download`, `merge`, or `conflict` actions.
They leave local files and sync metadata unchanged. File-content merge decisions
are deferred to an actual sync.

`doctor` reports connectivity and authentication, the cache location, pending
sync phase, and unresolved-conflict count. It does not verify every cached object,
repair storage, or prove backup integrity.

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
rustsync-cli conflicts ./notes
```

Choose a version:

```sh
rustsync-cli resolve notes.txt --keep-local ./notes
rustsync-cli sync ./notes
```

Or keep the remote side:

```sh
rustsync-cli resolve notes.txt --keep-remote ./notes
rustsync-cli sync ./notes
```

If you want a manual merge, edit the original local file to contain the desired
result, then resolve with `--keep-local` and sync.

For a deletion conflict, the deleted side has no file to inspect. Choosing that
side removes the path, or leaves it absent. The conflict listing can still name
a remote-copy path even when the remote side is a deletion and no copy exists.

## Replace local contents deliberately

To discard local changes and use the remote snapshot:

```sh
rustsync-cli sync --discard-local --yes ./notes
```

Back up anything you want to keep first. This mode removes local-only files,
applies remote contents, clears unresolved conflicts, and updates local sync
state without publishing. An empty remote workspace means an empty local working
tree apart from its root `.rustsync` directory.

## Manage devices

Follow the [second-device walkthrough](../README.md#add-another-device) for
request, approval, bootstrap, and initial sync. Use a fresh directory on the
joining device and keep its pending identity until bootstrap succeeds.

Check fingerprints through a channel you trust before approving a request.
Device names are labels, not proof of identity.

The default `member` role can synchronize and list devices. Choose `owner` only
for a device that should approve others and deliver workspace keys. To inspect
current membership:

```sh
rustsync-cli device list ./notes
```

An owner can promote or demote an active device, or revoke its access:

```sh
rustsync-cli device set-role <device-id> owner ./notes
rustsync-cli device set-role <device-id> member ./notes
rustsync-cli device remove <device-id> ./notes
```

Use the device ID from `device list`. Promote another owner before removing or
demoting the last owner. Removal blocks subsequent server requests, including
after a restart; it does not erase files or keys already held by that device.

## Recover from errors

| Symptom | Next step |
| --- | --- |
| Connection refused or timeout | Check that the server is running and that every network command uses the correct `--server-url`. The client timeout is 30 seconds per request. |
| `init` failed while contacting the server | Read its cleanup message. Network failures attempt to remove newly created local metadata; other failures can leave it behind. Preserve existing files before changing metadata or retrying setup. |
| Bootstrap says the identity is missing | Run `device request` in that same pending directory, then obtain approval before bootstrapping. |
| Device is not active or lacks permission | Check `device list` from an owner and confirm the intended request was approved. Members cannot approve devices. |
| Approval succeeded but key-envelope delivery failed | Approval and key delivery are separate requests. Preserve the pending identity and the error details. The CLI has no dedicated redelivery command; retrying approval may report that the request is no longer pending. |
| Authentication timestamp rejected | Check the device and server clocks. Signed requests permit a five-minute timestamp difference. |
| Workspace changed during synchronization | Finish editing, then rerun `sync`; inspect any conflicts it reports. |
| Remote head changed twice during publication | Rerun sync after other writers finish. Do not use discard-local merely to bypass a race. |
| A pending phase remains after interruption | Preserve `.rustsync`, run `doctor`, and retry normal sync. Pending state is a checkpoint, not a guarantee that every interrupted filesystem write can be rolled back. |
| Unsupported workspace entry | Remove or relocate symlinks and special files from the directory being synced. Only regular files and directories are supported. |
| Object too large | Keep individual files and encrypted manifests below the 1 MiB object limit. There is no chunking or CLI limit override. |
| Missing or corrupt local cache or server storage | Preserve a copy of the affected data before attempting recovery. Restore a known-good backup; the client has no general repair command. |

Do not delete `.rustsync` as a routine repair step. It contains the device's
private identity, workspace keys, cached file versions, and reconciliation state.
See [backup and restore](server.md#backup-and-restore).
