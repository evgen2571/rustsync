# Internals

[Documentation](README.md) · [Backup and restore](deployment.md#backup-and-restore)

The protocol defines encrypted objects and workspace state shared by clients and
the server. Local caches and server catalogs support that protocol but have
separate persistence rules. These formats are pre-release implementation details.

## HTTP API

Routes are registered in [server API modules](../crates/rustsync-server/src/api/mod.rs).
Bodies use JSON except blob and manifest payloads, which use binary RSOB frames.
In this table, `{w}` is the workspace ID.

| Method and path | Operation |
| --- | --- |
| `GET /health` | Public HTTP availability check |
| `POST /workspaces` | Create workspace using the submitted owner's signed request |
| `GET`, `PUT /workspaces/{w}/head` | Read head or conditionally publish a manifest |
| `GET`, `PUT /workspaces/{w}/blobs/{blob_id}` | Transfer encrypted file data |
| `GET`, `PUT /workspaces/{w}/manifests/{manifest_id}` | Transfer encrypted manifest |
| `POST /workspaces/{w}/devices/join-requests` | Submit a request signed by the joining identity |
| `GET /workspaces/{w}/devices/join-requests` | Owner lists pending requests |
| `POST /workspaces/{w}/devices/join-requests/{request_id}/approval` | Owner applies signed approval |
| `GET /workspaces/{w}/access/state` | Read device membership and key grants |
| `POST /workspaces/{w}/access/events` | Apply a signed access change |
| `PUT /workspaces/{w}/keys/{key_id}/envelopes/{device_id}` | Owner delivers a recipient key envelope |
| `GET /workspaces/{w}/keys/{key_id}/envelopes/{device_id}` | Recipient retrieves its envelope |

Signed requests carry `x-rustsync-device-id`, `x-rustsync-timestamp`,
`x-rustsync-nonce`, and `x-rustsync-signature`. The
[canonical request types](../crates/rustsync-protocol/src/auth/canonical.rs) and
[HTTP client](../crates/rustsync-client/src/transport.rs) define signing and encoding.
The [security guide](security.md#requests-and-server-trust) explains authentication
and its bootstrap exceptions.

Head updates supply `expected_revision` and `manifest_id`. A stale revision
returns a conflict with the current head; it does not replace the winner.
Object IDs must match submitted bytes. Repeated uploads of identical canonical
objects are accepted. The server cannot validate plaintext references inside an
encrypted manifest.

The client has a 30-second request timeout and bounds encrypted-object downloads
to 1 MiB. The server bounds request bodies to 1 MiB. Enrollment and key-envelope
delivery are separate requests, not a single transaction.

## Local workspace

```text
workspace/
  ...user files...
  .rustsync/
    workspace.toml
    device.identity.toml
    devices.toml
    access.toml
    keys/
      keyring.toml
      <key-id>.key
    manifest.json
    sync-state.json
    blobs/
      <plaintext-content-hash>
```

| Entry | Purpose |
| --- | --- |
| `workspace.toml` | Workspace ID, local device ID, default key configuration, and optional server URL |
| `device.identity.toml` | Local private device identity |
| `devices.toml`, `access.toml` | Local device and access metadata |
| `keys/` | Workspace encryption keys and keyring records |
| `manifest.json` | Last staged snapshot used by local status and transfer operations |
| `sync-state.json` | Reconciliation base and its applied revision, observed/published remote state, pending phase, and conflict records |
| `blobs/` | Cached plaintext file versions addressed by content hash |

The staged snapshot and reconciliation base have different jobs. Staging makes
file bytes available for transfer and merging; it does not itself publish a
remote revision. A failed sync can leave a staged snapshot and a pending phase.

Conflict records name the original local path and remote-copy path. A
`remote_deleted` flag distinguishes an intentional remote deletion from a copy
that should exist. The registry is local; ordinary conflict-copy files are part
of the synchronized tree.

Sync-state writes use a temporary file followed by rename. Local file application
and manifest writes are not a transaction over the entire working tree. Keep the
metadata and cache when retrying an interrupted operation.

During enrollment, `.rustsync` initially contains the pending identity rather
than a complete workspace. Bootstrap verifies the envelope before moving that
metadata to `.rustsync.bootstrap-backup` and creating the enrolled workspace.
Successful setup removes the backup; a cleanup or rollback failure can leave it
behind and reports its path. Preserve it until the identity is safely recovered.

## Server layout

```text
storage-root/
  .rustsync-server.lock
  workspaces/
    <workspace-id>/
      state.sqlite3
      state.sqlite3-wal       # when present
      state.sqlite3-shm       # when present
      objects/
        ab/
          cd/
            <content-hash>.enc
```

The [SQLite schema](../crates/rustsync-server/migrations/0001_initial.sql) contains
`schema_migrations`, `objects`, `workspace_head`, `access_state`, `join_requests`,
and `key_envelopes`. The head and access state each use a singleton row.

Each workspace has its own SQLite database for mutable state, key envelopes,
and the object catalog. SQLite uses WAL mode with `synchronous=NORMAL`. The object tree stores
immutable encrypted payload bytes, including file blobs and manifests.

An ID such as `blob_abcdef...` or `manifest_abcdef...` loses its type prefix for
the physical filename. The first two and next two hash characters provide the
fan-out directories. Blob and manifest IDs remain distinct protocol types even
though they share the physical object tree.

The storage root has one independent owner. Clones of an `IndexedFsStorage`
handle share its lock, database registry, and publication mutex. A second
independent open fails until the last owner handle drops. The retained lock
file is not itself evidence that an owner is still running.

## Publication and integrity

Head publication uses a conditional SQLite update against the expected revision.
A stale writer receives the current head without overwriting it.

Publication writes a temporary object file, syncs its contents, and links it to
the canonical destination before inserting its SQLite catalog row. The server
re-reads and validates the canonical file after a publication attempt, including
when another writer won the race.

| Stored state | Read or existence-check result |
| --- | --- |
| Valid object file and matching catalog row | Return the object or report it present |
| Valid file without a catalog row | Recreate the catalog row on access |
| Catalog row without its file | Report storage corruption |
| File bytes do not match the requested content ID | Report storage corruption |
| Catalog kind, hash algorithm, or encrypted size disagrees | Report storage corruption |
| Neither file nor catalog row exists | Report the object missing |

Repeated publication succeeds only if the canonical bytes match the submitted
payload. Reading or updating a workspace head also validates its referenced
physical manifest. A head pointing to a missing or corrupt manifest is not
advertised as usable. This validation does not decrypt the manifest or verify
all of the file blobs it references.

The server syncs new object-file contents but does not fsync parent directories
after link or rename. SQLite's `NORMAL` setting and the separate filesystem
operations do not provide a fully crash-consistent commit across both stores.
Lazy catalog repair covers valid files without rows; it is not a general repair
or rollback mechanism.

## Encrypted object frame

Remote blobs and manifests use RSOB version 1:

```text
magic[4] | version:u8 | algorithm:u8 | key_id_len:u16be | nonce_len:u8 |
key_id:utf8[key_id_len] | nonce[nonce_len] | ciphertext[remaining]
```

The magic is `RSOB`. Version is `1`; algorithm tag `1` is XChaCha20-Poly1305.
All remaining bytes belong to the ciphertext, so extensions require a new
frame version. The content ID hashes the full frame, not the plaintext.

Clients reject malformed frames, unsupported versions or algorithms, non-RSOB
bytes, and legacy JSON object envelopes. There is no fallback object decoder.
The complete frame must fit within the 1 MiB object limit.

## File chunks

A file entry uses `remote_blob_id` when its encrypted contents fit in one object.
Larger files instead use `remote_chunk_ids`, an ordered array of encrypted blob
IDs. Each chunk has its own nonce and authentication tag. The sender subtracts
the current key's framing overhead from the object limit to choose the plaintext
chunk size. Empty files still use one encrypted blob.

The receiver authenticates each chunk, concatenates their plaintext in manifest
order, and checks the complete file's size and SHA-256 hash before applying it.
Missing, corrupted, reordered, or truncated chunks fail the pull. A file entry
cannot specify both reference formats. Existing single-blob manifests remain
readable; older clients cannot read chunked files and should be upgraded together.

Unchanged files reuse their entire chunk list. Editing a file uploads all of its
chunks again; this is not a delta-transfer format. The local cache continues to
store complete plaintext files by content hash. The encrypted manifest itself
is not chunked and must remain below the object limit.

## Format changes

The server accepts its current SQLite v1 schema and validates it when first
opening a workspace database. It rejects unsupported or inconsistent schemas
instead of migrating them.

Before upgrading builds, retain independent plaintext backups and
copies of the old local and server state. For an incompatible format change,
create fresh workspaces and re-enroll devices, then import the files you intend
to keep. Do not delete the only copy of data as part of a reset.
