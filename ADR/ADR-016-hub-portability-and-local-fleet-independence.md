# ADR-016: Hub Portability -- Local-Fleet Independence from the Community Hub

**Status:** Proposed · **Date:** 2026-09-08 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** this conversation

## Context

Jack asked the question directly: if the Office Hours community element of OH Hive never takes off -- or simply stops being funded -- does a member still end up with something useful, or does the whole thing stop working? He wants that answered honestly before deciding whether the team's effort should weight toward Supabase-side infrastructure or toward the applications themselves.

The honest answer, checked against ADR-001 and ADR-004 rather than assumed: as architected today, everything stops. OH Hive already uses peer-to-peer transport (libp2p, ADR-004) -- but only for bulk data (artifacts, model weights, checkpoints), specifically to keep that traffic off Supabase's egress limits. State does not travel that way. Projects, cards, leases, the Honey ledger, membership, and even *which regional server is currently allowed to coordinate the network* are deliberately centralized in one Supabase project (ADR-001's "hub and source of record"), and the p2p overlay itself bootstraps by reading a list of live regional servers from that same project. There is currently no path that keeps anything running if that one project goes away.

That centralization was the right call for the reason ADR-001/004 give: the Hive marketplace has to arbitrate between members who don't inherently trust each other -- whose lease is valid, whose Honey balance is real, who gets paid -- and that is a genuine multi-party trust problem. Solving it without any central source of truth means real distributed consensus, the kind of engineering a cryptocurrency needs. This ADR does not propose building that. Honey is already a deliberately closed-loop, non-cashable credit (ADR-012, decision 11) specifically to avoid the regulatory and technical weight that would otherwise require -- paying the consensus tax anyway, on top of that, would spend a large amount of engineering to protect an asset that was designed from day one not to need it.

The insight that unlocks a real answer: a member's own fleet (ADR-015's `execution_mode = 'local'`) has none of that multi-party trust problem. There's no second party to arbitrate between -- it's one member's machines, one member's data, one member's call. A single trusted process can safely be the source of truth for that member's own state without any election, consensus, or Byzantine-fault tolerance, because nothing there is being adjudicated between mutually distrusting parties. That is the actual lever: not decentralizing the Hive marketplace, but making sure the *local-fleet* path was never welded to one specific hosted project in the first place.

## Decision

### 1. Two dependency tiers, not one architecture

The Hive marketplace (`execution_mode = 'hive'`, per ADR-015) keeps exactly the architecture ADR-001 and ADR-004 already describe. This ADR does not change it, does not attempt to decentralize the ledger or coordinator election, and explicitly recommends against building consensus infrastructure for Honey given the closed-loop design already avoids needing it.

Local-fleet mode (`execution_mode = 'local'`) gets a different property: **the hub it talks to is configuration, not a hardcoded assumption.** The same schema, the same RPC shapes, the same worker/scheduler code run unmodified against whichever Postgres instance a member's install is pointed at.

### 2. Three hub tiers, same schema

1. **Community hub** -- today's shared Supabase project. Required for anything that touches the Hive marketplace: posting a project for other members, spending or earning shared Honey, the per-project forum, browsing what the community is building. Unchanged by this ADR.
2. **Personal cloud hub** -- a separate, smaller Postgres instance not tied to the community project's continued existence. Could be a member's own minimal self-hosted instance, or a lightweight offering Happy Jack Media runs independently of the cost of running the full community marketplace. Gives "access my fleet from my phone, from anywhere" without any dependency on whether Office Hours keeps going.
3. **Fully local hub** -- a Postgres instance running on one of the member's own paired machines. The rest of that member's fleet reaches it over the LAN or the Cloudflare Tunnel automation already built into the desktop app (shipped earlier this session). Zero externally-hosted dependency at all. This is the floor: even if Happy Jack Media stopped operating entirely, a member who chose this tier keeps their local-fleet setup working exactly as before.

All three speak the same schema and the same `public.hive_*` RPC surface local mode already uses. Switching tiers is a connection-config change to the desktop app, not a different codebase, and not a rewrite of ADR-015's local execution engine.

### 3. Keep local mode's schema usage boring, on purpose

For this portability to be real rather than aspirational, local-mode's Postgres usage has to stay deliberately simple: plain tables, "is this my own row" row-level security, no ledger, no multi-tenant complexity. This is already true of what ADR-015 specified (local projects post no ledger entries and are only claimable by the owner's own nodes) -- this ADR makes it an explicit constraint going forward: nothing added to the local-mode code path should assume Supabase-specific behavior (a particular Edge Function, a particular Auth provider quirk, a particular Realtime feature) that a self-hosted Postgres + PostgREST instance, or a bundled embedded instance, couldn't also serve.

### 4. The desktop app should be able to run a local hub itself

For tier 3 to be genuinely turn-key -- not "go set up your own Postgres server" -- the desktop app should be able to bundle and manage a local Postgres instance directly, the same way it already manages the local execution engine and the Cloudflare Tunnel. A member with one or a few of their own machines should get full local-fleet capability out of the box, with no external account, no hosting bill, and no separate install step beyond running the app.

## Consequences

### Positive
- Answers Jack's actual question directly: a member who only ever uses their own fleet keeps a working product regardless of what happens to the Office Hours community or to Happy Jack Media's hosting of the shared project.
- Costs far less than the alternative. Building genuine decentralized consensus to protect the Hive marketplace itself would be a large, separate engineering program solving a problem the closed-loop Honey design already sidesteps; making local mode hub-portable is a scoped, boring-on-purpose piece of work by comparison.
- Reinforces the answer to "Supabase or applications": the applications (desktop app, local-fleet management) become the durable product; the hub underneath is deliberately kept swappable rather than load-bearing for the thing members actually depend on day to day.
- Reuses infrastructure already built this session (the Cloudflare Tunnel automation) rather than inventing new connectivity plumbing for tier 3.

### Negative
- Three hub tiers means three things to test and support instead of one, even though the code is shared.
- Bundling and managing a local Postgres instance inside the desktop app is real packaging work, platform by platform -- not free just because the schema is portable.
- A "personal cloud hub" tier, if Happy Jack Media offers it as a product rather than leaving it to self-hosting, is an ongoing hosting/support commitment layered on top of the community project.
- Members who start on the community hub and later want to move to a personal or local one need their data to come with them; this ADR does not yet specify how.

### Risks & mitigations
- **"Boring schema" discipline erodes over time** as new features get built against whichever hub is fastest to prototype against (usually the community Supabase project), quietly reintroducing Supabase-specific assumptions into local mode. Mitigation: treat local-mode PRs that add a Supabase-specific dependency as a review flag, the same discipline already used this session for the overload-ambiguity and RLS patterns.
- **Tier 3 (fully local hub) packaging turns out to be harder than expected** on some platform (Windows bundling of Postgres has real edge cases). Mitigation: tier 2 (personal cloud hub) is a legitimate fallback that still meets "not dependent on the community project" even if bundling a local database proves too heavy for v1.
- **A member on a non-community hub can't reach the Hive marketplace at all**, which could read as a bait-and-switch if not communicated clearly. Mitigation: the tier is a member's explicit choice, made visible in the app, not a silent fallback; promotion to the Hive (ADR-015 S3) should say plainly if it requires moving to the community hub first.

## Open questions
- What does authentication look like on a personal or fully local hub? Today's model is Supabase Auth (GoTrue) tied to the community project. A single-owner local hub arguably doesn't need OAuth at all -- trusting the machine's own OS session may be sufficient -- but this hasn't been designed. (Open.)
- How does a member migrate local-mode project data between hub tiers (community to personal, personal to fully local, or back)? Needs export/import tooling against the shared schema; not designed here. (Open.)
- Does Happy Jack Media offer the personal cloud hub as an actual product/tier, or is it self-hosting-only guidance? This is a business decision as much as a technical one and directly affects how much this ADR is worth building ahead of demand. (Open.)
- Exactly how does the desktop app bundle/manage a local Postgres -- an embedded binary shipped per-platform, a lightweight alternative that speaks enough of the Postgres wire protocol, or a guided setup that installs Postgres proper? (Open -- S4 states the requirement, not the implementation.)
- If a project is promoted to the Hive (ADR-015 S3) while running on a personal or fully local hub, does promotion require first migrating that project's data into the community hub, and does this ADR's open migration-tooling question block that path? (Open -- flagged as a dependency between this ADR and ADR-015, not resolved by either.)

## Related
- ADR-001-hub-and-source-of-record (the centralization this ADR does not change for Hive-marketplace mode)
- ADR-004-p2p-overlay-and-regional-servers (clarifies: existing p2p is bulk-data-only; state centralization there is deliberate and stays)
- ADR-012-scope-and-roadmap (the closed-loop, non-cashable Honey decision that makes decentralized consensus unnecessary to build)
- ADR-013-cost-capacity-and-hosting (Supabase tier limits that originally motivated keeping bulk data off it -- same cost-consciousness applies here)
- ADR-015-local-workstation-and-hive-promotion (the execution_mode='local' tier this ADR makes hub-portable; the Cloudflare Tunnel automation this ADR reuses for tier 3)
