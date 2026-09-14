# ADR-025: The Fully Local Hub — Private Fleet Data Never Leaves the Member's Machines

**Status:** Accepted · **Date:** 2026-09-13 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** this conversation

## Context

Jack asked directly, after this session shipped a fix that gates *who can read* a private-fleet (`execution_mode = 'local'`) project's data: he wants private fleet to never touch the community Hive's cloud context at all. Not "restricted by row-level security," not "encrypted at rest in the same database" — the data should physically live on the member's own machines and never be uploaded to Supabase in the first place.

Checked against what's actually built (see the research this ADR is based on, folded in below rather than kept as a separate doc): `execution_mode = 'local'` today changes exactly two things — who may claim a card (owner's own nodes only) and whether Honey gets charged (never). It changes nothing about *where* the data lives. Every project, card, checkpoint, and output — local or hive mode — is a row in the same Supabase Postgres tables, now correctly access-controlled, but still 100% cloud-resident. That gap is real and is what this ADR closes.

ADR-016 (2026-09-08) already named the right target — a "fully local hub" tier, Postgres running on one of the member's own paired machines, zero externally-hosted dependency — but left it Proposed, with the two hardest questions explicitly marked open: what authentication looks like without Supabase Auth in the loop, and what actually stores the data. This ADR resolves both, scoped narrowly to what a private, single-owner fleet actually needs — not the full three-tier hub-portability program ADR-016 sketched, which remains future work if a "personal cloud hub" product ever gets prioritized.

Also relevant, confirmed by direct code reading (`crates/ohhive-core/src/hub.rs`): `HubClient` is a concrete struct hardwired to Supabase's PostgREST/Edge-Function wire format, used directly in ~23 files. There is no `Hub` trait today. Coordinator election is a pure Postgres row-lock, not a real consensus algorithm — irrelevant here anyway, since ADR-016's own insight holds: a single member's own fleet has no multi-party trust problem, so no election is needed. One machine is simply always the hub for that member's local-mode work.

## Decision

### 1. Scope: the private-fleet *data plane* only

This ADR covers exactly the surface `execution_mode = 'local'` cards actually need at runtime: claiming a card, completing it, checkpointing agent-loop state mid-run, failing/releasing a lease, sub-delegation (`spawn_child_card`/`wait_on_child`), and reading a member-configured MCP server's config for a tool step. It does **not** cover: Honey/ledger (local mode already posts none), regional-server replication or backup/ledger-archive (community-hub-only machinery), coordinator election (not needed — see above), or BYOK key storage/chat memory/release notes/feature-and-bug-report submission (member-account conveniences, not project data; they stay on the community hub for now, since a member's BYOK provider keys and account-wide chat memory aren't the "private fleet" concern Jack raised — a project's actual work product is). Cloud LLM calls a member's coding agent makes with their own API key (Anthropic/OpenAI/Nous, direct) are unaffected either way — that's a call to the model provider, not to Hive's backend, and was never the thing Jack objected to.

### 2. A `Hub` trait, with `SupabaseHub` (today's `HubClient`, renamed) as one implementation

```rust
#[async_trait::async_trait]
pub trait Hub: Send + Sync {
    async fn claim_card(&self) -> Result<Claim, HubError>;
    async fn complete_card(&self, card_id: Uuid, content: &str, model_id: Option<&str>, usage: Usage) -> Result<Completion, HubError>;
    async fn checkpoint(&self, card_id: Uuid, step: u32, state: &serde_json::Value, usage: Usage) -> Result<serde_json::Value, HubError>;
    async fn fail_card(&self, card_id: Uuid, reason: &str) -> Result<serde_json::Value, HubError>;
    async fn release_card(&self, card_id: Uuid, reason: &str) -> Result<serde_json::Value, HubError>;
    async fn spawn_child_card(&self, parent_card_id: Uuid, key: &str, title: &str, modality: &str, inputs: &str, acceptance: &str, required_capabilities: serde_json::Value) -> Result<SpawnedCard, HubError>;
    async fn wait_on_child(&self, card_id: Uuid, child_card_id: Uuid) -> Result<serde_json::Value, HubError>;
    async fn mcp_server_config(&self, server_id: Uuid) -> Result<McpServerConfig, HubError>;
    // check_in / heartbeat / check_out / get_schedule stay on Hub too, since Worker's loop
    // calls them regardless of what a claimed card's project mode turns out to be — a local
    // hub answers them trivially (this machine is always "checked in" to itself).
    async fn check_in(&self, caps: &Capabilities, region: Option<&str>) -> Result<serde_json::Value, HubError>;
    async fn heartbeat(&self, prev_rtt_ms: Option<u64>) -> Result<(String, u64), HubError>;
    async fn check_out(&self) -> Result<String, HubError>;
    async fn get_schedule(&self) -> Result<Option<serde_json::Value>, HubError>;
}
```

`SupabaseHub` wraps today's `HubClient` 1:1 — every method above already exists on it verbatim; this is a rename plus a trait `impl` block, not a rewrite. Every other `HubClient` method (BYOK keys, chat memory, channel posts, regional-server/replication/backup/ledger-archive, coordinator election, artifact upload/fetch, pairing) stays exactly where it is, called directly, unaffected — those are Hive-marketplace or member-account concerns outside this ADR's scope, and forcing them into the trait would bloat it for no reason. `Worker<'a>`'s `hub: &'a HubClient` field becomes `hub: &'a dyn Hub`; every call site inside `worker.rs`/`coder.rs`/`tools.rs` that only needs a trait method keeps working unchanged, and the small number of call sites that need something Hive-specific (e.g. spawning a code session's cloud brain call) keep a direct `HubClient`/`SupabaseHub` reference alongside.

### 3. `LocalHub`: SQLite, one designated machine, LAN-only

A new `LocalHub` implementation of the same trait, backed by an embedded SQLite database (new dependency — nothing like it exists in the Rust core today) holding a deliberately boring mirror of the relevant `hive.*` tables: `projects`, `cards`, `card_outputs`, `checkpoints`, `leases`, plus a `local_node_keys` table for §4 below. One of the member's own paired machines is the fleet's local hub for that project — chosen when the project is created (default: the machine that created it), not elected or negotiated, because there is no multi-party trust problem to arbitrate (ADR-016's own reasoning, directly reused). If that machine is the only one working the project, `LocalHub` runs in-process, no network involved at all. If a second paired machine needs to claim cards from the same private-fleet project, the hub machine also runs a small local-only HTTP server (same trait, served over `axum` or equivalent, LAN-bound by default) that the other machine's `LocalHub` client half talks to — reusing the Cloudflare Tunnel automation already built into the desktop app (ADR-013) for the case where "LAN" isn't actually the same network. No data in either case ever reaches Supabase: not the project row, not a single card, not a checkpoint, not an output.

### 4. Local-only node pairing: no GoTrue, no Supabase account, in the loop

Resolves ADR-016's first open question. A single-owner fleet doesn't need OAuth — it needs "prove you're standing at both machines." Pairing a second Mac into a fully-local fleet: the hub machine generates a short numeric code and a `hive_nk_`-shaped node key locally (same format as today's Supabase-minted keys, sha256-hashed at rest, just written to `local_node_keys` instead of `hive.node_keys`); the joining machine's owner types that code in; the hub machine hands back the raw key over the same local connection. No Supabase Auth session, no member JWT, no cloud round-trip, anywhere in this flow. This intentionally mirrors the trust model of AirDrop/Tailscale device pairing (physical presence at both ends, once), not OAuth device-authorization.

### 5. Existing local-mode Supabase data is not migrated by this ADR

Projects already created under today's Supabase-backed `execution_mode = 'local'` keep working exactly as they do now — this ADR does not retroactively move anything. A member choosing the fully-local hub does so for *new* private-fleet projects going forward. Export/import tooling to move an existing local-mode project's history off Supabase is real work and is explicitly out of scope here (ADR-016's own open question 2, still open) — flagged, not solved, so it isn't quietly forgotten.

## Consequences

### Positive
- Directly answers what Jack asked for: a private-fleet project created under the fully-local hub never has its project/card/checkpoint/output data leave the member's own machines, full stop.
- The `Hub` trait is scoped to exactly what local execution needs (§1), not ADR-016's entire three-tier program — a real, shippable slice instead of an open-ended platform rebuild.
- `SupabaseHub` is a rename-and-wrap of working code; hive-mode behavior has zero regression risk from this change by construction.
- Reuses the Cloudflare Tunnel automation already shipped (ADR-013) for the multi-machine-not-on-one-LAN case, rather than inventing new connectivity plumbing.

### Negative
- SQLite is a new dependency and a second schema to keep conceptually in sync with `hive.*` (by hand — there's no shared migration tooling between the two). Drift between them is a real, ongoing maintenance cost.
- `LocalHub`'s LAN server is new attack surface on the member's own network, even though it's narrower than a full Postgres instance — needs its own auth review before it ships (the node-key check must be as strict as `hive.verify_node_key` is today; §4's local key format reuses the same hashing, not a weaker scheme).
- A private-fleet project on the fully-local hub is invisible to the community Hive by construction (correctly — that's the point), which means "promote this to the Hive" (ADR-015 §3) requires the still-unbuilt migration tooling from §5 before it can work for these projects. Flagged, not solved here.

### Risks & mitigations
- **Trait-ification of `Worker`'s `hub` field touches the same files (`worker.rs`, `coder.rs`) reserved as Loki's in the ongoing Sif/Loki split.** Mitigation: this ADR's Decision §2 change ships as one small, mechanical, behavior-preserving commit (rename + trait + field-type change only, no logic change) verified by a full `cargo build`/`cargo test` pass before `LocalHub` work starts against it — see the continuity-log handoff for exactly how this is sequenced.
- **SQLite schema drift from `hive.*` over time.** Mitigation: keep `LocalHub`'s schema deliberately smaller than `hive.*` (§1's scope), not a full mirror — fewer columns to drift.
- **A member expects "fully local" to also cover BYOK keys/chat memory and is surprised those still touch Supabase.** Mitigation: the Settings UI (future work, not in this ADR) must say plainly which project mode a member is in and what stays cloud-resident regardless (account-level conveniences) vs. project data (fully local).

## Open questions
- Should a fully-local project's BYOK-key lookup also move local (a member's own Anthropic/OpenAI/Nous key, stored on-device instead of Supabase Vault) for members who want *zero* cloud dependency of any kind, not just zero project-data upload? Jack's stated concern was project data specifically; this is a natural follow-on, not decided here. (Open.)
- Multi-machine discovery: this ADR specifies manual entry (LAN IP/hostname or Cloudflare Tunnel URL, typed once during pairing) rather than mDNS/Bonjour auto-discovery, matching ADR-016's "boring schema, on purpose" discipline. Worth revisiting once the manual flow is live and its friction is felt for real. (Open.)
- ADR-016's tier 2 ("personal cloud hub," HJM-operated or self-hosted, not on the member's own machine) is untouched by this ADR — still Proposed, not decided for or against. (Open, deferred.)

## Related
- ADR-016-hub-portability-and-local-fleet-independence (the tier this ADR actually builds; resolves its two open questions for the single-owner case)
- ADR-015-local-workstation-and-hive-promotion (the `execution_mode='local'` tier this ADR gives real data-residency to)
- ADR-013-cost-capacity-and-hosting (the Cloudflare Tunnel automation reused for tier-3 multi-machine reachability)
- ADR-004-p2p-overlay-and-regional-servers (confirms state was never meant to travel over the community p2p overlay; this ADR doesn't change that, it gives local mode its own separate path instead)
- `docs/SIF-PERSONAL-FLEET-RECOMMENDATIONS-2026-09-13.md` (Sif's review; flagged private-project visibility, fixed same day this ADR was written, as a precondition for private fleet mattering at all)
