# Storage and persisted-data policy

This document describes the current filesystem-backed server storage format. It
is a pre-release development format, not a compatibility promise.

## Layout

For a storage root and workspace ID, the server uses this layout:

```text
<storage-root>/
  workspaces/
    <workspace-id>/
      state.sqlite3
      state.sqlite3-wal       # may exist while SQLite WAL mode is in use
      state.sqlite3-shm       # may exist while SQLite WAL mode is in use
      objects/
        ab/
          cd/
            <content-id-without-type-prefix>.enc
```

`state.sqlite3` is workspace-local mutable state and an object catalog. It
holds object metadata as well as the workspace head, access state, and join
requests. SQLite is opened in WAL journal mode with `synchronous=NORMAL`.

`objects/` holds the immutable encrypted payload bytes. Blob and manifest
objects share this private physical tree, but remain distinct typed concepts in
the protocol and public routes. For an ID such as `blob_abcdef...` or
`manifest_abcdef...`, the type prefix is removed for the filename and the
remaining ID is fanned out by its first two and next two characters:

```text
objects/ab/cd/abcdef....enc
```

The server treats an object payload as opaque bytes. It validates the typed
content ID against those exact bytes, but does not decode or decrypt RSOB
envelopes, receive workspace secret keys, or inspect plaintext/local paths.
The local workspace code owns encryption and decryption.

## Object catalog and repair behavior

Object publication writes the immutable file before inserting its SQLite
catalog row. A newly written temporary file is `sync_all`ed before it is linked
to its final object path.

For object reads and existence checks, the physical file is consulted first:

* A present file is rehashed against the requested typed ID. If valid, the
  server inserts its catalog metadata when absent. Thus a valid object file
  without a row is repaired lazily when it is read or checked for existence.
* A present file whose content does not match its path/ID is storage
  corruption, not a valid object.
* Catalog insertion checks that an existing row agrees on object kind and
  encrypted size; disagreement is an error.
* A missing file currently produces the ordinary missing result for the object
  operation (`BlobNotFound`/`ManifestNotFound` for reads and `false` for
  existence checks), even if a stale catalog row might exist. The implementation
  does not look up a catalog row before reporting that absence, so it does not
  currently distinguish a row-without-file from an object that was never
  cataloged.

Updating a workspace head first checks that the proposed manifest file exists;
that check also revalidates the manifest bytes and can backfill its catalog row.
The current `get_head` path reads the SQLite head record without rechecking the
referenced manifest file. Operators must therefore treat a persisted head and
its manifest objects as a single backup/restore unit.

## Encrypted-object format and reset policy

Remote encrypted objects use RSOB version 1 only. Its canonical frame is:

```text
magic[4] | version:u8 | algorithm:u8 | key_id_len:u16be | nonce_len:u8 |
key_id:utf8[key_id_len] | nonce[nonce_len] | ciphertext[remaining]
```

The magic is `RSOB`; version 1 currently supports algorithm tag `1`
(XChaCha20-Poly1305). All remaining bytes are ciphertext, so an extension to
the frame requires a new version. Content IDs hash the complete serialized
remote frame, not decrypted data.

Writers emit RSOB v1. Readers accept only valid RSOB v1: JSON envelopes,
non-RSOB bytes, malformed frames, unsupported versions, and unsupported
algorithm tags are rejected without a fallback parser.

RustSync is pre-release and its development data is disposable. After an
incompatible persisted-format change, delete the affected local workspace and
server storage data and initialize fresh state. The server supports the current
SQLite v1 schema only; it validates that schema when a workspace database is
first opened and rejects non-current or inconsistent schema state rather than
importing or migrating it.

## Process ownership and concurrency

There is currently no process-level storage-root lock and no
`.rustsync-server.lock` file. Do not run multiple independent server processes
against the same storage root as a supported deployment model.

Within one process, `IndexedFsStorage` handles for the same root share an
in-memory mutex that serializes object publication. That mutex is not visible
to another process and is not a substitute for cross-process coordination.

## Backup, restore, and durability

A restorable workspace requires both its SQLite state and its immutable object
files. Back up the complete storage root, including every workspace's
`state.sqlite3`, any SQLite-managed WAL/SHM sidecar files present at backup
time, and `objects/` tree. Copying only a live `state.sqlite3` file is not a
safe SQLite WAL backup and omits the payloads in all cases.

For a straightforward consistent backup or restore:

1. Stop the server. (Alternatively, use a SQLite-aware backup plus a
   coordinated filesystem snapshot.)
2. Copy or restore the full storage root, preserving each workspace database,
   its live SQLite sidecars when applicable, and its `objects/` directory
   together.
3. Start the server and exercise the restored workspaces; workspace databases
   are opened and validated lazily on first use.

The implementation syncs newly written object-file contents before publishing
them, but does not fsync parent directories after link/rename. SQLite uses WAL
with `synchronous=NORMAL`, not `FULL`. Consequently this branch does not claim
power-loss durability or a fully crash-consistent filesystem/SQLite commit
protocol. A valid object file left behind before its catalog row can be
backfilled on later object access; other interrupted or externally modified
states can remain unavailable or be reported as corruption.
