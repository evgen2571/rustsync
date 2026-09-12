# Deployment

[Documentation](README.md) · [Security](security.md) · [Internals](internals.md)

## Run with Docker

With Docker Engine and the Compose plugin installed, run from the repository root:

```sh
docker compose up -d
curl --fail http://127.0.0.1:3000/health
```

The root [Compose file](../compose.yaml) builds the [server image](../Dockerfile) and starts it at
`http://127.0.0.1:3000`, the default client URL. The image runs as UID/GID 10001,
and the `server-data` named volume retains SQLite state and encrypted objects
across container replacement. Only localhost can reach the published port.
The image listens on `0.0.0.0:3000` inside the container and stores data in `/data`.
Compose drops Linux capabilities, disables privilege escalation, and restarts the
service unless stopped. A bind mount replacing the named volume must be writable
by UID/GID 10001.
The image health check tests HTTP availability, not every stored object.

```sh
docker compose logs server
docker compose down                 # Stop containers; retain the data volume.
docker compose up -d --build        # Rebuild after updating the source.
```

`docker compose down --volumes` deletes the stored server data. Stop the server
before backing up the complete volume; see [backup and restore](#backup-and-restore).
For remote access, put HTTPS or a private tunnel in front of the HTTP listener.

## Start and configure

After [installing from source](../README.md#quick-start), run:

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

After configuring an HTTPS proxy, clients use its reachable hostname:

```sh
rustsync --server-url https://sync.example.com sync ./notes
```

`sync.example.com` is a placeholder for your proxy hostname. A private tunnel
can instead expose the loopback HTTP address to enrolled devices.

The server serves HTTP and has no built-in TLS configuration. For access over
an untrusted network, terminate HTTPS at a reverse proxy or use a private tunnel.
Keep proxy request paths and signed request bodies intact. Device signatures and
object encryption do not encrypt all HTTP metadata. See [security](security.md).

## Check the service

```sh
curl --fail http://127.0.0.1:3000/health
rustsync doctor ./notes
```

The health endpoint checks that the HTTP service responds. `doctor` also makes
an authenticated workspace-head request. Neither command checks every workspace
database or stored object. Workspace databases are opened and validated lazily.

Startup failures are printed to stderr and exit unsuccessfully. Common causes
are an occupied TCP port, an unwritable data directory, or another process
owning the storage root.

## Limits

A workspace can have at most 128 pending device join requests. Workspace
creation has no administrator allowlist; use network access controls for a
private deployment. See [request authentication](security.md#requests-and-server-trust).

The current server request-body limit is 1 MiB. The client also limits each
downloaded encrypted object to 1 MiB. The limit includes encryption framing and
authentication overhead. Files exceeding the available payload are split into
separately encrypted chunks and reconstructed by the receiving client.
The encrypted manifest must also fit, which bounds the number and total path
length of files in one workspace. These limits have no CLI configuration flag.

Budget client memory for complete reconstructed files and cached versions used
during reconciliation. Chunking bounds network objects, not workspace memory.

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
SQLite, object files, and local working-tree changes. The [storage reference](internals.md)
describes the guarantees and failure states more precisely.
