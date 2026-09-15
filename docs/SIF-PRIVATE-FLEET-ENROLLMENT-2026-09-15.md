# Verified Private Fleet enrollment — implementation and rollout

Sif your friendly Codex Agent — 2026-09-15.

Implements the identity/enrollment foundation in item 2 of `SIF-FLEET-IDENTITY-AND-PRIMARY-PLAN-2026-09-15.md`. This is distinct from selecting a remote primary (item 3), running deliveries on secondary hosts (item 4), and replicated primary handover (item 5). Do not add all six machines as separate primaries and call that one shared fleet.

## What the code does

- `supabase/migrations/20260915130000_private_fleets.sql`: a private fleet registry owned by `auth.users.id`, independent of `hive.members`. Authenticated users may read their own records. Only the service role may create/change records. No community schema or invitation guard changes.
- `supabase/functions/private-fleet-enroll/`: validates the supplied access token with Supabase `auth.getUser`, rejects anonymous users and identities without Google/Apple, checks fleet ownership, then signs a narrowly scoped five-minute Ed25519 approval. The body cannot supply its own owner. Server errors and secrets are not echoed to clients.
- `apps/web/app/private-fleet/enroll/page.tsx`: public sign-in, private fleet creation, explicit device review, and copying the returned approval. No `RequireMember` wrapper. This does not grant OHG access.
- `crates/ohhive-core/src/local_hub/enrollment.rs` and schema v9: persisted authority identifier, configured issuer trust, expiring key-bound challenges, replay ledger, atomic verified owner binding. Remote clients can request/complete enrollment only with an already locally paired node key. Initial issuer trust is installed only through trusted local administration, never by an RPC caller.
- `crates/ohhive-ffi/src/local_hub.rs`, `bots.rs`, `lib.rs`: native enrollment entry points, independent private enrollment status, offline private Bots opening and per-operation revalidation. Revoked keys and changed local credentials fail closed. Community pairing remains a separate status.
- Native `PrivateFleetEnrollmentView.swift`, Settings, Setup and HiveStore: first-primary approval controls; nonmembers can complete setup and open local Bots without starting the community worker. The interface explicitly identifies remote-primary connection as pending work.

## Trust and wire contract

A local paired session requests `enrollment_challenge`, receiving authority UUID, local node UUID, SHA-256 fingerprint of its actual key, 256-bit random nonce and expiry. The raw key never goes to the platform signer. Request replacement invalidates the previous challenge.

The owner signs in on the website, selects a fleet they own, compares the device fingerprint and authority identifier with their local app, and explicitly approves. Names are presentation only; UUID/fingerprint/nonce form the binding.

The envelope is `{payload, signature}`, both unpadded base64url. `payload` is UTF-8 JSON. Ed25519 signs the exact bytes of `hive.private-fleet.enrollment.v1\n` followed by the base64url payload string. Claims bind issuer, key ID, audience `hive-private-fleet-enrollment`, stable platform subject, fleet, authority, node, credential fingerprint, nonce, unique assertion ID, issue time and expiry. Rust verifies the signature before parsing claims; it enforces a maximum five-minute lifetime and 30-second future issue-time tolerance. Deno never signs beyond the local challenge's expiry.

The authority then checks its stored fleet/owner, actual authenticated key, current challenge, existing owner binding, and replay ledger in one SQLite transaction. Rejection does not consume a valid challenge. Bootstrap cannot replace an existing authority's trust or owner. Subsequent Bots operations use the persisted owner without needing an online sign-in check.

Trust comes from platform deployment configuration, not from keys embedded in an approval. A different issuer cannot claim ownership merely by copying a UUID. This increment deliberately does not implement key rotation, account recovery, automatic propagation of platform logout/deletion to offline fleets, or a multi-owner sharing policy. Local device revocation is enforced immediately by the authority. Browser sign-out alone does not revoke a previously authorized device.

## Deployment prerequisites — not executed in this change

1. Apply the new migration to the intended Supabase project. Confirm its existing auth/signup hooks permit ordinary Google/Apple users to obtain accounts without OHG membership; the shared project's live configuration was not changed here.
2. Create a dedicated Ed25519 signing key, keeping its private PKCS#8 DER material in deployment secrets. Do not reuse a node key, provider API key, or Supabase service-role key as the signing key.
3. Configure the function secrets: `PRIVATE_FLEET_SIGNING_KEY_PKCS8` (base64url PKCS#8), `PRIVATE_FLEET_SIGNING_KEY_ID`, `PRIVATE_FLEET_ISSUER` (canonical HTTPS issuer), `PRIVATE_FLEET_ALLOWED_ORIGINS` (comma-separated exact website origins). Existing `SUPABASE_URL` and `SUPABASE_SERVICE_ROLE_KEY` are also required.
4. Deploy the function and website. The handler authenticates every POST itself using `auth.getUser`; preserve its checks regardless of gateway JWT settings. Allow the website's `/auth/callback` in Google/Apple/Supabase redirect configuration. Authentication in the shared project may already be configured, but that is not proof that public private enrollment works live.
5. Distribute the matching **public** raw 32-byte Ed25519 key as base64url through trusted app/platform configuration: `HIVE_PRIVATE_FLEET_PUBLIC_KEY`, `HIVE_PRIVATE_FLEET_KEY_ID`, `HIVE_PRIVATE_FLEET_ISSUER`. The app reads the normal local node configuration, so a supported installer can supply this without asking end users to manage cryptographic keys. Do not ask users to copy public keys from enrollment payloads.
6. Rebuild Rust FFI and regenerate Swift bindings before building the desktop. Schema v9 is a real forward migration: old schema-v8 binaries refuse the upgraded store. No production store migration was performed by the isolated tests.
7. Start with one approved primary and a new test fleet. Use Settings → Private Fleet (or Setup), generate a request, approve on the website, finish in Hive, then verify local Bots. Test a real nonmember Google account and Apple account separately. This live acceptance gate remains outstanding.

## Existing installation considerations

Private Bots uses the actual local node identifier. Older native Bots profiles may prefer the community node UUID. Do not rewrite all old conversations or agent host IDs automatically: inspect/reassign those profiles explicitly when moving an existing installation to the private identity path. Existing owner bindings cannot be reassigned to a different account by enrollment.

The initial native flow configures this Mac as an authority. It does not connect a secondary to an existing primary. Core/HTTP remote pairing plus signed enrollment exists and is tested; remote endpoint selection, lifecycle, native remote conversation operations, host delivery authorization and synchronization are still queued. Do not turn the current first-primary screen into a misleading “add all computers” wizard until those pieces are integrated.

## Verification

- Final core suite: 154 passed, one ignored; includes real loopback HTTP enrollment and Bots access.
- Updated enrollment tests: five passed, including persisted authority/identity across restart, wrong owner/fleet/authority/node/credential, invalid signature/issuer/key/audience/time, challenge replacement/expiry, replay, revocation, and remote pairing.
- Deno service/auth-return tests: six passed, covering actual Ed25519 signing/verification, server-derived owner, foreign fleet refusal, request bounds/CORS, expiry and anonymous/provider identity filtering.
- Embedded PostgreSQL (PGlite) executes the actual migration against isolated auth fixtures: owner isolation, anonymous denial, missing identity, service-only creation and denied client mutations passed. `scripts/test_private_fleet_rls.mjs` accepts the installed PGlite module path; production was not contacted.
- Web TypeScript and function entry-point type checks passed. Migration static guards passed. Native bridge Bots tests: five passed.
- Native regenerated bindings compiled; three BotsModel tests and the release build passed. No visual or real-provider sign-in success is inferred from compilation/unit tests.
- OAuth callback return paths now reject off-site/script destinations, preserving the private enrollment route.

Next: explicit shared-authority selection and offline/pending state, then host-authorized remote delivery. Keep the existing authority authoritative; never silently fall back to an unrelated local database if it is unavailable.


## Item 3 foundation added after enrollment verification

`crates/ohhive-core/src/local_hub/authority.rs` introduces a credential-bearing, serializable explicit remote selection and a selected Bots client. Reconnection pins authority, fleet, owner and node IDs. The client has no local database fallback. Enrollment identity receipts now include the authority ID. The selection belongs only in protected node configuration; it must not be logged or stored in the knowledge library.

The real HTTP test round-trips this configuration, rejects each mismatched identity field, sends a message, stops the server, verifies failed reads/retry without fallback, restarts the same authority and confirms the retried request returns the original message with one history entry. Revocation still rejects the open session and reconnect.

This is **client infrastructure**, not completed native primary selection. Remaining item-3 work: persist the choice through native settings, manage the private listener/pairing lifecycle, route every native Bots operation through the selected authority, expose offline/pending state, and preserve draft/request IDs across reconnect. Item 4 must then authorize/route runtime delivery to the actual secondary host. Item 5 provides replication and planned primary transfer; endpoint selection is not promotion and does not copy data.
