# Native shared-primary selection

Sif your friendly Codex Agent — 2026-09-15.

Implements queue item 3's native connection path on top of the signed private enrollment foundation. This selects an existing authority; it does not replicate data, promote a secondary, or implement secondary execution.

## User flow

Settings → Private Fleet now includes **Primary computer**. The same controls appear during first-run account setup.

On a verified first primary, choose **Share from this Mac**, enter its explicit private LAN IP and port (for example `192.168.1.143:8787`), then **Start sharing**. **Create pairing code** produces an eight-digit code valid once, for five minutes. This is a separate private LocalHub listener. It neither starts nor changes the community regional server/tunnel. Wildcard/public binds are refused. Sharing is opt-in per app launch; automatic service startup/discovery is not implemented here.

On the secondary, choose **Connect to a primary**, enter that computer's URL and pairing code, then request approval. Approve the connection request on the enrollment website using the same fleet as the primary; compare the fingerprint and authority ID. Paste the approval and choose **Use this primary**. The secondary verifies the platform signature and exact pending challenge itself before accepting the authority's enrollment result. The prior deployment/signing-key prerequisites still apply.

The selected authority, fleet, owner and local node are checked before opening a remote Bots session. Selection lives in `private-primary.json` beside `node.env`, saved via a private temporary file, fsync and atomic replacement (0600 on Unix). This file contains the device credential and must not be committed, logged, replicated as library content, or shared. A pending enrollment is held only in memory and must be restarted after app exit.

A secondary's native Bots session contains a remote transport and no local database. All current roster, metadata, conversation, message and thread-root lookup calls use that transport. A corrupt or unreadable saved selection fails closed. Changing the saved selection invalidates existing sessions; a failed remote call cannot choose a local history. Same-primary reconnect preserves drafts and request IDs; account/primary changes clear the previous session's private drafts/state. Late send receipts from an invalidated session cannot clear a newer draft/retry.

The UI states where history is stored and shows connection/pending errors. Messages can be written to the primary for its local agent. Secondary execution remains explicitly unavailable until item 4; this change does not claim a full working distributed agent team. Saved chats/drafts are not a crash-persistent outbox; process exit still loses unsaved drafts.

## Changes

- `crates/ohhive-ffi/src/private_fleet.rs`: independent listener lifecycle, serialized connection changes, pairing, approval, exact binding verification, persisted choice and status.
- `crates/ohhive-ffi/src/bots_storage.rs`: mutually exclusive local/remote backends; synchronous facade runs on existing blocking workers, with network work driven by the owned runtime.
- `crates/ohhive-ffi/src/bots.rs`: selects remote before opening a local database, checks selected identity, routes operations through the backend and refuses remote drain until delivery routing exists.
- `local_hub/bots.rs` / `transport.rs`: owner-scoped single-message lookup for thread validation. No arbitrary caller owner IDs are accepted at the server.
- `PrivatePrimaryView.swift`, HiveStore, Settings, Setup, BotsModel/BotsView: connection controls, remote status, storage label and retry-preserving reconnect.

## Verification and remaining gates

Core regression suite: 154 passed, one ignored. Native bridge real-HTTP suite: six tests passed, including remote roster/rename/DM/join/message/thread lookup, stable request deduplication, foreign-account denial, revocation, offline behavior and no local backend. Atomic preference-file test passed with replacement and 0600 checks. All five Swift BotsModel tests passed, including reconnect retry preservation and primary-change isolation. The native Swift release build and Rust release FFI build passed; Swift bindings were regenerated. Diff whitespace checks passed.

No live production enrollment, physical second-machine test, migration deployment or tunnel exposure was performed. Apple/Google enrollment service deployment remains a prerequisite. Before adding the whole fleet, finish item 4 (host-authorized remote delivery), then run a small two-computer pilot. Item 5 owns snapshot/journal replication and planned primary transfer; item 6 owns private tunnel/discovery. Do not reinterpret the connection picker as a primary-promotion switch.
