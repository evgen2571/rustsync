# Security

[Documentation](README.md) · [Deployment](deployment.md) · [Internals](internals.md)

RustSync protects workspace contents from a storage server that does not hold
the workspace key. Devices perform encryption, decryption, and reconciliation.
The server authenticates requests and controls access to encrypted objects.
This model depends on trusted local devices and careful enrollment.

Enrollment invites contain only the canonical workspace ID and server URL.
They carry no keys and grant no access. They are unsigned, reusable connection
details, so obtain them through a trusted channel and verify the server address.
Owners must still compare the requesting device fingerprint through a trusted
channel and explicitly approve enrollment before delivering an encrypted key.

## Contents and keys

File data and manifests use XChaCha20-Poly1305 with a fresh random nonce for each
encryption. Encrypting the manifest hides its relative paths, plaintext hashes,
file sizes, and blob references. Object IDs are SHA-256 hashes of the complete
encrypted frame. Before applying remote files, the client checks object IDs,
authenticates ciphertext, validates paths, and verifies reconstructed file sizes
and hashes.

An owner delivers the shared workspace key in a recipient envelope using X25519,
HKDF-SHA256, and XChaCha20-Poly1305. The envelope carries the sender's signature.
The server stores the envelope without receiving the plaintext key. See the
[envelope implementation](../crates/rustsync-core/src/access/envelope.rs) and
[object encryption](../crates/rustsync-core/src/encryption.rs).

All active members can read and write workspace contents. Owners additionally
manage devices and keys. These are workspace-wide roles, not per-file permissions.
Verify a joining device's fingerprint through a trusted channel before approving
it. A display name does not establish identity.

Removing a device revokes its server access and key grants. It cannot revoke
plaintext or keys already downloaded, and the CLI does not rotate the shared key.
An owner cannot remove or demote the last active owner.

## Requests and server trust

Ed25519 signatures cover the method, request path and query, body hash, device ID,
timestamp, and nonce. The client generates a fresh 128-bit nonce using operating
system randomness. The server accepts timestamps within five minutes of its
clock and rejects nonce reuse within its in-memory replay window.

Workspace creation authenticates the owner supplied in the creation request.
Join requests authenticate the requesting identity before it becomes a member.
Neither operation requires a server administrator account or invitation secret.
Restrict network access when the server is intended for a closed group; device
permissions do not provide server-wide registration control or storage quotas.

The server can observe workspace IDs, public device keys, roles, join requests,
access revisions, encrypted object IDs and sizes, and traffic timing. It can
withhold data or return an older head. Replay tracking disappears on restart,
and the client does not provide complete malicious-server rollback protection.
Encryption does not guarantee availability, freshness, or secrecy of metadata.

The server binary speaks HTTP. Use an HTTPS proxy or a private tunnel for remote
access. Preserve the signed path, query, and body when forwarding requests.

## Local trust boundary

The working tree and `.rustsync/blobs/` contain plaintext. `.rustsync` also holds
private device identity and workspace keys. RustSync has no passphrase unlock or
local encryption-at-rest layer. Protect the device and its backups with operating
system access controls and disk encryption as needed.

Do not commit `.rustsync`, expose it through a web server, or copy an owner's
identity to enroll another device. Use [device enrollment](usage.md#add-another-device)
to create separate identities. Endpoint compromise exposes that device's files
and keys regardless of server security.

## Supported behavior and limits

| Area | Behavior |
| --- | --- |
| File types | Regular files and directories; symlinks and special files are rejected unless excluded from scanning |
| Paths | Normalized relative UTF-8 paths using `/` in manifests |
| Metadata | File contents and directory structure; no permissions, ownership, ACLs, or extended attributes |
| Timestamps | Unchanged local files retain their modification time; received files do not restore original filesystem timestamps |
| Merge | Compatible line replacements in equal-line-count UTF-8 text; other competing changes preserve conflicts |
| Transfer | Manual sync; no background watcher or delta transfer within changed files |
| Size | Each encrypted chunk and the unchunked manifest must fit within 1 MiB; files are reconstructed in memory |
| History | Current snapshot only in the CLI; no history browser or restore command |
| Compatibility | Persisted formats are pre-release; no automatic migration contract |
| Platforms | Linux is covered by CI; other platforms are not verified by that CI |

Ignore rules govern new local paths, not previously tracked secrets. Read
[ignore behavior](usage.md#ignore-development-files) before the first sync.
Keep nested RustSync workspaces outside the tree: scanner exclusions do not
promise to preserve nested metadata during snapshot application.

Run one sync or resolution command per local workspace at a time. The client
checks for concurrent local edits but does not lock other programs out. Local
application is not atomic across a directory, and server publication spans
SQLite and separate object files without a power-loss durability guarantee.
Keep independent [backups](deployment.md#backup-and-restore) and preserve metadata
when following [recovery instructions](usage.md#recover-from-errors).
