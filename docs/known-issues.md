# Known issues

[README](../README.md) · [Recovery guidance](usage.md#recover-from-errors)

These issues were reproduced by launching the actual client and server
executables while checking the documentation. They affect the current code;
the documentation rewrite does not include their fixes.

## Repeated publication fails with the SQLite server

The `rustsync-server` executable uses `IndexedFsStorage`. Initial publication
advances a workspace from revision 0 to 1, but subsequent publication attempts
can fail with a message such as:

```text
remote head changed from revision 1 to 1 while publishing; rerun `rustsync sync`
```

This was reproduced with one writer, without a competing sync. To reproduce
against a running server, use a fresh workspace and space commands to avoid the
separate nonce issue:

```sh
rustsync-cli init ./repro
printf 'first\n' > ./repro/note.txt
sleep 1
rustsync-cli sync ./repro
printf 'second\n' > ./repro/note.txt
sleep 1
rustsync-cli sync ./repro
```

The SQLite head-update statement selects an insert row only when the expected
revision is zero. With a nonzero expected revision, it selects no row, so its
conflict-update branch never executes. See
[SQLite head updates](../crates/rustsync-server/src/storage/sqlite.rs).

Repeatedly retrying does not fix this case. Preserve local edits and metadata;
do not discard local changes or reset server storage to work around it. Further
publication requires a code fix. The normal multi-device editing and conflict
workflows described in the guides are blocked when they need that publication.
Initial upload, reading an existing snapshot, enrollment, and deliberate local
replacement can still be exercised.

The existing HTTP and device-bootstrap tests use `FsStorage`, while the shipped
server uses `IndexedFsStorage`. Follow-up coverage needs to exercise multiple
successive revisions through the shipped backend, including two-device edits.

## Rapid CLI processes reuse request nonces

Each client process begins its request counter at the same value. The nonce is
that counter combined with the current timestamp in seconds. Two processes
using the same workspace and device in the same second can therefore send the
same nonce. The server correctly rejects the repeated value with:

```text
request replay detected
```

The client error code is `ReplayDetected`. This can happen when pasting commands
or running a script, even when no request was intentionally replayed.

For sequential commands, wait at least one second after a network command
finishes before starting another for the same device:

```sh
rustsync-cli remote-status ./notes
sleep 1
rustsync-cli doctor ./notes
```

Inspect partial state before retrying a failed initialization or enrollment
operation. Command spacing is a temporary workaround, not a fix for concurrent
processes. A code fix needs nonces unique across processes and a test that
launches separate executables. See
[nonce generation](../crates/rustsync-client/src/transport.rs).
