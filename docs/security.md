# Security and limitations

[README](../README.md) · [Server operation](server.md)

## What is encrypted

Devices encrypt file contents and manifests with XChaCha20-Poly1305 before
uploading them. The manifest contains plaintext-relative paths, file sizes,
content hashes, and remote blob references; its remote representation is encrypted.
The server stores those payloads as opaque objects.

Object IDs use SHA-256 over the complete encrypted frame. The client verifies
object IDs, decrypts and authenticates payloads, and validates file hashes, sizes,
and paths before applying a downloaded snapshot. The server checks object IDs
against stored bytes but does not decrypt their contents.

Workspace keys stay on devices. During enrollment, an owner seals the shared
workspace key for the recipient using X25519, HKDF-SHA256, and XChaCha20-Poly1305.
The server stores and delivers that encrypted key envelope.

## What the server can see

Encryption does not hide workspace IDs, device records and public keys,
membership roles, join requests, access revisions, encrypted object IDs and
sizes, or request timing. The server also controls availability and which head
it returns.

Workspace requests use Ed25519 device signatures. Authentication binds the
request method, path, body hash, device ID, timestamp, and nonce. The server
checks permissions and rejects timestamps outside a five-minute window.
Each request uses a fresh 128-bit nonce from the operating system's random
generator. Separate CLI processes do not share a timestamp-based counter.

Replay tracking is in memory; it does not survive a server restart. These
mechanisms should not be described as complete protection against a malicious
server replaying or withholding historical state.

Use HTTPS through a proxy or a private tunnel for remote connections. The server
binary itself provides an HTTP listener.

## Protect local devices

The working tree and `.rustsync/blobs/` cache contain plaintext file contents.
The metadata directory also contains private device identity and workspace keys.
Local storage is not encrypted at rest by RustSync and has no passphrase unlock
workflow. Protect it with operating-system permissions and disk encryption as
appropriate for your environment.

Do not commit `.rustsync` to another repository, serve it as web content, or copy
an owner's identity to enroll another device. Use the request/approve/bootstrap
workflow to create a distinct identity.

Owners and members both receive access to the shared workspace contents.
Owners additionally manage access and keys. Compare fingerprints before approving
a device; a display name is not an identity check.

`device remove` revokes server access and the device's key grants. Removal cannot
erase plaintext or keys already obtained by that device. The CLI has no key-rotation
command, so removal does not provide retroactive secrecy.

## Supported files and behavior

| Area | Current behavior |
| --- | --- |
| File types | Regular files and directories; staging rejects symlinks and special files |
| Paths | Relative, normalized, UTF-8 paths with `/` separators in manifests |
| File metadata | Contents and directory structure are synchronized; executable modes, ownership, ACLs, and extended attributes are not represented |
| Modification times | Recorded in manifests, but not restored as original filesystem timestamps |
| Ignore rules | No user-configurable ignore rules or `.gitignore` support; the scanner excludes entries named `.rustsync` |
| Merge | Equal-line-count UTF-8 text with compatible line replacements; other cases preserve conflicts |
| Transfer | Manual whole-object synchronization; no watcher, chunking, or delta transfer |
| Object size | 1 MiB per encrypted file object or manifest, including overhead |
| History | The CLI exposes the current snapshot, not version browsing or historical restore |
| Compatibility | Pre-release persisted formats; no migration promise |
| Platforms | CI exercises Linux; other platforms have no CI-backed support guarantee |

Keep nested RustSync workspaces outside a synchronized tree. The scanner's
`.rustsync` exclusion is not a general preservation rule for nested metadata
during snapshot application.

A successful sync and a passing test suite do not establish backup durability.
Local application is not a transaction across the whole directory, and the
server does not guarantee survival of every write after power loss. Keep
independent backups and review [recovery guidance](usage.md#recover-from-errors)
before resetting metadata.
