# Track E: owner-bound Bots transport

Sif your friendly Codex Agent — 2026-09-15.

Implemented the roster/messaging transport handoff and its owner-identity addendum (`14bccab`). The addendum supersedes the original request to leave an owner-spoofing hole. The temporary development gate and the test expecting cross-account access have been removed; ordinary builds now enforce a persisted account binding.

## Implemented

`LocalHub` wrappers, dispatch arms and `RemoteLocalHub` methods cover agents list/create/update/archive; conversations list/create/join; messages list/send; and mark-read. Existing HTTP bearer authentication, request limits, TLS rules and error mapping are reused. No delivery/handoff/admin-binding RPC was added.

Schema version 8 adds nullable `nodes.owner_member_id` through `owner_schema.sql`. Existing nodes migrate with NULL ownership and cannot use Bots until verified. Existing Vault/card behavior remains unchanged. `LocalHubStore.set_node_owner` is trusted local administration only, rejects missing nodes/nil identity, allows idempotent same-owner binding, and refuses rebinding a key to a different account. It is never dispatched over HTTP. Account reassignment requires an explicit revoke/re-enroll workflow, not silent reassignment of existing credentials.

All Bots wrappers revalidate the local node key and read its persisted owner. Agents list/update/archive and mark-read no longer take caller-selected owner/user IDs. Creation still uses existing serialized draft structs for compatibility, but overwrites draft.owner from the verified session; that input is never authoritative. Conversation creation checks that its coordinator belongs to this account. Explicit actors must resolve to this account (User ID equality or AgentProfile owner); conversation IDs and recipients must also belong to this account, then normal core membership checks apply. A paired device is account-authorized, not restricted to one agent within that account; this follows the addendum's intended trust model.

This actor check matters: conversation membership alone could not prevent a caller claiming another member's or agent's ID. The revised tests explicitly reject those cross-account actor claims.

## Native bootstrap and the two node IDs

`HiveNode.bots_open` already verifies `whoami` against the configured hub. It now calls a local helper after that successful response, opens the existing vault reader identity, and binds **that local reader node ID** to the verified member. The cloud `whoami.node_id` is not the local SQLite reader ID; stamping the cloud UUID directly would update no row. `BotsSession.host` remains the cloud node ID for the existing runner/agent placement contract.

Only the native device's own local reader is automatically stamped here. A remotely paired device on the authority still requires trusted enrollment/verified owner binding at that authority. Merely opening Bots on a secondary stamps its own database, not the primary's. Do not claim complete remote onboarding from this patch. Future pairing must carry a verified assertion bound to the target local credential, not expose set_node_owner as a client-callable setter.

## Verification

Real loopback HTTP test pairs two clients, confirms unbound rejection, binds both to one owner, creates/updates/lists agents, creates/joins/lists a DM, sends/reads a message, retries without duplication, marks read and archives. Invalid and revoked keys fail, including an already-open local session after revocation. A third account cannot read/post/join/archive/update the first account's objects or impersonate its user/agent. A malicious create draft cannot select another owner.

Separate integration test exercises the library without unit-test configuration: unbound access fails, verified binding works, same-owner rebinding is idempotent, different-owner/missing-node/nil-owner binding fails, and revocation still applies. Version-7 migration test preserves an existing node with NULL ownership. Current core/FFI test totals are in continuity.

## Remaining work and guidance requested from Claude

- Client authority selection, trusted remote device enrollment, tunnels and UI wiring remain follow-ups. No server was deployed and no physical fleet acceptance was run.
- Remote workers still need a way to consume deliveries from the authority. Roster/message sharing alone cannot run an agent on another computer: its old local worker reads a different database. The original handoff intentionally excludes delivery RPC, but the later execution phase needs host-bound claims/results or authenticated forwarding to the runtime host. Keep that distinct from freely allowing clients to claim any agent's work.
- Core draft update validation/DM uniqueness, cancellation and reply/completion atomicity remain inherited concerns.
- User direction on public private-fleet identity and primary/secondary takeover is captured in `SIF-FLEET-IDENTITY-AND-PRIMARY-PLAN-2026-09-15.md`. Do not make OHG community membership a prerequisite for private fleet identity.

Changes remain in the shared checkout for Claude's integration. This is verified transport/account isolation, not finished fleet-wide Bots UI or failover.
