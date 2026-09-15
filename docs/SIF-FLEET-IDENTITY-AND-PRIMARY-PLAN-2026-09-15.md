# Private fleet identity and primary/secondary hosts

Sif your friendly Codex Agent — 2026-09-15. User requirements and proposed next implementation steps; not implemented by the transport patch.

## User direction

Jack wants private fleets available to people outside the invite-only OHG community. Apple/Google sign-in may establish their identity without granting community membership. Every eligible machine should have the code to act as primary, with one current primary and other secondaries; users should be able to choose/promote a primary from a dropdown. Automatic failover was not requested.

## Separate identity from community membership

Use a stable platform identity with linked Apple/Google sign-in identities. Device ownership is a separate persisted authorization binding; OHG membership is a separate invitation/entitlement. Never treat an email address or a UUID supplied by the client as proof. Different identity issuers need explicit namespaces; a UUID from an arbitrary configured community hub is not automatically a global platform identity.

The existing `/pair` page already offers Apple/Google OAuth. However, `hive.pair_claim` and `hive.pair_peek` in `supabase/migrations/20260905000003_pairing.sql` require `hive.is_member()`. That helper requires an active row in `hive.members` (`20260905000001_hive_schema.sql`). Current account sign-in therefore does not make nonmembers eligible to pair. This is a code-level observation, not an audit of deployed database configuration.

Recommended follow-up: a private-fleet enrollment path for any verified platform identity, with a separate device registry/ownership grant if the community's node schema requires membership. Preserve the existing community invitation checks. Private enrollment must not opt devices into compute contribution, accept community terms implicitly, or grant access to community projects. Sign-in screens should state “Set up your private fleet”; joining a community remains optional.

After authenticated owner approval, the authority binds the target local node credential to the verified identity. A server-verified, short-lived enrollment assertion should bind issuer, subject, target fleet/authority, target credential/node, nonce and expiry; replay must fail. Alternative trusted local approval is possible but must still establish the same owner, not take it on the joining device's word. Subsequent local RPC can use persisted bindings offline, with explicit revocation semantics. Decide the offline-only identity/recovery option separately if needed; do not pretend social sign-in works without an initial connection.

Primary references: [Supabase Apple sign-in](https://supabase.com/docs/guides/auth/social-login/auth-apple), [Google sign-in](https://supabase.com/docs/guides/auth/social-login/auth-google), [identity linking](https://supabase.com/docs/guides/auth/auth-identity-linking).

## Primary selection requires replicated state

Every eligible device can run the authority service, but only the current primary accepts authoritative writes. Keep a fleet ID, primary node ID, authority epoch, endpoint, last committed sequence and replica readiness/checksum. The dropdown lists the current primary and each secondary's eligibility, reachability and synchronization state. Do not offer promotion as successful merely because a hostname changed.

Start with planned manual handover while both devices are reachable:

1. Verify owner authorization and the target's readiness. Stream an application-consistent snapshot plus ordered journal to the target; verify schema compatibility, checksums and applied sequence.
2. Quiesce writes and delivery ownership on the old primary. Finish or explicitly checkpoint active work; transfer the final journal tail. Persist the old primary's demotion before enabling target writes.
3. Persist a new authority epoch and target primary, then reconnect clients. Writes carry stable request IDs and the expected authority epoch. A known-demoted primary rejects writes rather than maintaining an independent history.
4. Retain the old node as a secondary, with rollback/recovery evidence. Show success only after the target accepts a verified test transaction and clients discover the new epoch.

For forced takeover when the old primary is unreachable, a dropdown alone cannot prevent split brain. Add a quorum or trusted lease/witness protocol whose fencing survives network partitions before claiming safe automatic/unreachable-primary takeover. An isolated old primary must eventually lose write authority; learning a new epoch only after reconnect is insufficient. Until that exists, keep planned handover separate from explicit disaster recovery with disclosed potential data loss and reconciliation.

Replicate logical authoritative Bots/identity/grant state, not an actively open SQLite file through iCloud/Drive. The existing vault-host database also contains machine-specific vault paths and local job state; blindly copying all of it to another host is incorrect. Provider secrets and local subscription-process sessions must not be copied as ordinary chat data. Keep credentials device-scoped or use a designed encrypted transfer.

## Ordered work that can continue during Claude's pause

1. Finish owner-bound transport and adversarial tests (current work).
2. Design/implement verified remote enrollment and public private-fleet identity independently of OHG membership.
3. Add explicit authority selection and visible offline/pending behavior, retaining request IDs; no silent local fallback.
4. Add host-authorized delivery routing so an agent executes on its actual runtime host while conversations remain authoritative on the selected primary.
5. Add snapshot/journal replication and planned primary transfer, with crash/partition tests; then wire the primary/secondary dropdown.
6. Expose the separate private LocalHub tunnel and endpoint discovery. Do not reuse/overwrite the community regional-server tunnel.

Provider agents, rooms and mentions should then build on shared authority rather than independent per-machine stores. Keep this ordering subject to new user direction and Claude's active claims.
