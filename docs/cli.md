# Command reference

[Documentation](README.md) · [Usage](usage.md) · [Server options](deployment.md#start-and-configure)

The installed client executables are `rustsync` and `rustsync-cli`. Run `rustsync --help`
or append `--help` to a subcommand for its parser-generated usage.

```text
rustsync [--server-url URL] <command>
```

`--server-url` is global and defaults to `http://127.0.0.1:3000`. It can appear
before or after the subcommand. There is no client environment-variable or
saved-workspace setting for this option.

Workspace arguments default to `.`. Pass the workspace root, or run from that
root; commands do not search parent directories for a workspace.

## Workspace commands

| Syntax | Effect | Contacts server |
| --- | --- | --- |
| `init [WORKSPACE]` | Creates local identity, keys, metadata, and a new remote workspace | Yes |
| `status [WORKSPACE] [--json]` | Reports local changes, device role, revisions, counts and live server reachability | Yes |
| `remote-status [WORKSPACE]` | Fetches the current remote head revision and manifest ID | Yes |
| `sync [WORKSPACE]` | Reconciles files with the remote workspace | Yes |
| `sync --dry-run [WORKSPACE]` | Reports planned actions without changing local files, local sync metadata, or the remote head | Yes |
| `sync --discard-local --yes [WORKSPACE]` | Replaces local contents with the remote snapshot and clears conflicts | Yes |
| `conflicts [WORKSPACE]` | Lists locally recorded unresolved conflicts and their paths | No |
| `resolve PATH --keep-local [WORKSPACE]` | Keeps the local side of a conflict and removes the remote copy | No |
| `resolve PATH --keep-remote [WORKSPACE]` | Applies the remote side of a conflict | No |
| `doctor [WORKSPACE]` | Checks workspace metadata, identity, key material, cached content hashes, and an authenticated remote-head request; reports pending work and conflicts | Yes |

`PATH` in `resolve` is relative to the workspace root and must match a path
listed by `conflicts`. Supply exactly one of `--keep-local` and `--keep-remote`.
Resolution changes local files and metadata; run `sync` afterward to publish it.
If the chosen side deleted the path, keeping that side means keeping the deletion.

`--discard-local` requires `--yes` and cannot be combined with `--dry-run`.
It does not publish a new remote revision. If the remote workspace is empty,
it removes local workspace contents apart from the root `.rustsync` metadata
and ignored local files.

A dry run classifies files that need a merge without downloading all file
contents. A reported `merge` can become a conflict during an actual sync.

`status` compares local changes against its saved reconciliation base and fetches the remote
revision and access role. A matching working tree after a failed sync does not
prove publication succeeded. If the server cannot be reached or authentication
fails, status still reports local information, marks the server unavailable,
and exits unsuccessfully. A cached role is not proof of current remote access.

## Device commands

| Syntax | Effect |
| --- | --- |
| `device request WORKSPACE_ID [DIRECTORY] [--device-name NAME]` | Creates or reuses a pending device identity and submits a signed join request |
| `device list-requests [WORKSPACE]` | Lists pending requests, including device fingerprints; requires an owner |
| `device approve REQUEST_ID [WORKSPACE] [--role ROLE]` | Approves a pending request and delivers its encrypted workspace key; requires an owner |
| `device bootstrap WORKSPACE_ID [DIRECTORY]` | Uses the approved pending identity and delivered key to finish local setup |
| `device list [WORKSPACE]` | Fetches and lists devices, roles, status, and fingerprints |
| `device remove DEVICE_ID [WORKSPACE]` | Revokes an enrolled device's server access; requires an owner |
| `device set-role DEVICE_ID ROLE [WORKSPACE]` | Changes an active device to `owner` or `member`; requires an owner |

All device commands contact the server. `--role` accepts `member` or `owner`
and defaults to `member`. Both roles can read and write workspace contents.
Owners can also manage access and deliver keys.

The last active owner cannot be removed or demoted. Repeating an already-applied
removal or role change leaves the access revision unchanged. A removed device
cannot regain access through a role change.

The current CLI has no key-rotation, history, standalone push/pull, or staging command.

## JSON and progress

`status --json`, `sync --json`, and `doctor --json` write one JSON object to
stdout. Success includes `ok: true`; failures include `ok: false` and return a
nonzero exit status. A command failure includes `error`. An unavailable server
in status is described by `server.error` alongside the local status fields.

| Command | Main JSON fields |
| --- | --- |
| `status` | `workspace_id`, `device` with `id`, `name`, `role`, `local_revision`, `remote_revision`, `files`, `conflicts`, `pending_changes`, `pending_operation`, `server` with `reachable`, `error` |
| `sync` | `workspace_id`, `mode`, `observed_remote_revision`, `synced_revision`, `published`, `conflicts`, `plan`, `uploaded_blobs`, `uploaded_bytes`, `downloaded_blobs`, `downloaded_bytes` |
| `doctor` | `workspace_id`, `remote_revision`, `cached_objects`, `conflicts`, `pending_operation`, `checks` |

Unknown revisions are `null`. `conflicts` is a count in status and doctor, and
an array of paths in sync. A successful sync can preserve conflicts for manual
resolution, so automation must check this array. Transfer totals count encrypted
blob payloads, excluding manifests and HTTP overhead. No-op syncs report zero
blob transfers and `published: false`.
In a dry run, `synced_revision` is `null` because no synchronization was applied;
`observed_remote_revision` identifies the remote snapshot used for the preview.

Workspaces saved by older versions may report an unknown local revision until
their next successful sync. Status does not infer it from a later remote
observation. `pending_changes` counts changed paths, including directories.
The local revision tracks the remote snapshot applied to the local tree, even
if publishing remaining local edits subsequently fails. Both status and doctor
report `pending_operation` as `null` or an object with `phase` and
`observed_remote_revision`.

```sh
rustsync sync ./notes --json | jq '{synced_revision, uploaded_bytes, conflicts}'
rustsync doctor ./notes --json
```

JSON mode suppresses human progress. For text output, `sync --no-progress` hides
per-blob messages on stderr and retains the stdout summary. `sync --quiet`
hides both successful text outputs; errors still appear and fail the command.
`-q` is the short form of `--quiet`. `--json` and `--quiet` are mutually exclusive.
Failures, including parser errors, return exit status 1. When the arguments
contain `--json`, errors use the JSON envelope; otherwise they go to stderr.
Help and version requests exit successfully.

## Version and shell completions

`rustsync version` and `rustsync --version` report the client and protocol
versions. They work without a workspace or server. `completions SHELL` accepts
`bash`, `elvish`, `fish`, `powershell`, and `zsh`.

```sh
rustsync completions bash > rustsync.bash
source rustsync.bash
rustsync completions fish > ~/.config/fish/completions/rustsync.fish
rustsync completions zsh > _rustsync
```

Create the Fish completions directory first if needed. For Zsh, put `_rustsync`
in a directory on `$fpath` and initialize completions with `autoload -Uz compinit;
compinit` in your shell configuration. Generated completions target `rustsync`.

## Quoting paths

Quote a workspace or conflict path containing spaces:

```sh
rustsync resolve 'drafts/meeting notes.txt' --keep-remote ./notes
```

See [usage](usage.md) for enrollment, conflict resolution, and recovery workflows.
