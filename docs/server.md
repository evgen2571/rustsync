# Running the server

[README](../README.md) · [Storage reference](storage.md)

## Start and configure

After [installing from source](../README.md#install-the-commands), run:

```sh
rustsync-server
```

The process stays in the foreground. Stop it with Ctrl+C. Configuration comes
from command-line flags, then environment variables, then defaults.

| Flag | Environment variable | Default |
| --- | --- | --- |
| `--host` | `RUSTSYNC_HOST` | `127.0.0.1` |
| `--port` | `RUSTSYNC_PORT` | `3000` |
| `--storage-dir` | `RUSTSYNC_STORAGE_DIR` | `./server-storage` |

For a different local port and an explicit data directory:

```sh
rustsync-server --host 127.0.0.1 --port 4000 --storage-dir ./data
```

Equivalent configuration in a POSIX shell:

```sh
RUSTSYNC_HOST=127.0.0.1 RUSTSYNC_PORT=4000 RUSTSYNC_STORAGE_DIR=./data rustsync-server
```

Relative storage paths are resolved from the server process's working directory.
For a service manager, use an absolute storage path and a persistent directory
writable by the service account. Run one server process per storage root.

The server creates the storage directory if absent and takes an exclusive
advisory lock on `.rustsync-server.lock`. If startup reports that the root is in
use, stop the other owner or choose a different root. A leftover lock file alone
does not mean the root is locked; removing it is not a recovery procedure.

## Network access

The default listener accepts connections only from the local machine. To listen
on all IPv4 interfaces:

```sh
rustsync-server --host 0.0.0.0 --port 3000 --storage-dir ./data
```

Clients must use an address they can reach, not `0.0.0.0`:

```sh
rustsync-cli --server-url http://192.0.2.10:3000 sync ./notes
```

`192.0.2.10` is an example address; replace it with the server's address.

The server serves HTTP and has no built-in TLS configuration. For access over
an untrusted network, terminate HTTPS at a reverse proxy or use a private tunnel.
Keep proxy request paths and signed request bodies intact. Device signatures and
object encryption do not encrypt all HTTP metadata. See [security](security.md).

## Check the service

```sh
curl --fail http://127.0.0.1:3000/health
rustsync-cli doctor ./notes
```

The health endpoint checks that the HTTP service responds. `doctor` also makes
an authenticated workspace-head request. Neither command checks every workspace
database or stored object. Workspace databases are opened and validated lazily.

Startup failures are printed to stderr and exit unsuccessfully. Common causes
are an occupied TCP port, an unwritable data directory, or another process
owning the storage root.

## Limits

A workspace can have at most 128 pending device join requests.

The current server request-body limit is 1 MiB. The client also limits each
downloaded encrypted object to 1 MiB. The limit includes encryption framing and
authentication overhead, so a plaintext file of exactly 1 MiB will not fit.
The encrypted manifest must also fit, which bounds the number and total path
length of files in one workspace. These limits have no CLI configuration flag.

The client buffers whole objects and can hold many workspace files in memory
while reconciling. It does not stream or split large files into chunks.

The server has no CLI for storage quotas, object garbage collection, historical
restore, or migration. Immutable objects can accumulate after edits and failed
publications. Monitor storage usage and maintain backups.

## Backup and restore

A server backup needs the complete storage root. Each workspace's SQLite database,
any remaining WAL/SHM sidecars, and encrypted object files belong together.
Copying only a live `state.sqlite3` file is not a consistent backup.

For a straightforward backup:

1. Stop clients from syncing and stop the server.
2. Copy the entire storage root to a separate backup location.
3. Restart the server with the original storage root.
4. Verify the backup by starting a separate server against a restored copy and
   checking it with a separate copy of an enrolled local workspace.

To restore, stop the server and replace the full storage root from one consistent
backup. Start it, then check affected workspaces before resuming normal writes.
Restoring an older server snapshot can make clients reconcile with older content;
preserve current local copies first.

Also back up each local workspace, including its root `.rustsync` directory,
while its client is idle. The server backup contains encrypted data and recipient
key envelopes, but does not contain the private device keys needed to open them.
Treat local backups as secret material.

RustSync does not promise power-loss durability or an atomic transaction across
SQLite, object files, and local working-tree changes. The [storage reference](storage.md)
describes the guarantees and failure states more precisely.
