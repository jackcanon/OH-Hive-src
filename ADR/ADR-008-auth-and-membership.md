# ADR-008: Auth, Membership and Project Roles

**Status:** Proposed · **Date:** 2026-09-04 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** ADR-000 Q1–Q3, Q6, Q9–Q10, Q15; D1, D6, D8, D18, D20, D31, D35

## Context

OH Hive shares one Supabase project with Cmd Work (D9, D10) and inherits its identity layer unchanged: Supabase Auth with Sign in with Apple and Google, federated to a single user whose `public.profiles.id == auth.users.id` (Q9). The Hive does not introduce a second login, password store or user table. What it adds is *membership*: a `profiles` row is a person, but only a person with an active `hive.members` row is a Hive member, and only members may read Hive data (D8).

Membership is invite-only at launch (D1) and has exactly three on-ramps (D20): buy third-party API credit (which becomes $honey), register a computer as a compute node, or register regional server hardware. Any one qualifies, subject to an invite. The purchase path runs through Stripe with the hub acting as reseller: the member pays the Hive, the Hub buys provider credit, and provider API keys live only in Supabase Vault / Edge Function secrets, never on member nodes (D18).

Inside a project, three roles apply (D6): Owner (creates and dictates the project via the interviewer), Admin (curates cards, may bind dedicated compute), Follower (may suggest cards). Transparency is deliberately asymmetric: every active member can read every project and card (D8, D35), but writes are gated by `hive.project_roles`, and nothing is exposed to non-members or the public internet (D67). Compute contributors acquire no rights in the outputs they generate and must not redistribute `owner_only` material they can see (Q15), so a contributor Terms of Service is part of node registration.

## Decision

1. **Supabase Auth inherited as-is (D31).** Sign in with Apple and Google only; no email/password at launch. The web app (ADR-009) uses `supabase-js` with PKCE; the Tauri node app (ADR-010) uses the same provider flow through the system browser and a deep-link callback. Web SIWA requires a separate Apple **Services ID** and redirect URL registered for `ohghive.com`; this is a one-time portal task on the launch checklist.
2. **`profiles` is the identity; `hive.members` is the membership (D31).** `hive.members(profile_id pk → public.profiles.id, status, on_ramp, invited_by, invite_code_id, wallet_id, tos_accepted_at, created_at)`. No Hive table references `auth.users` directly; everything keys on `profiles.id`.
3. **Invite-only launch (D1).** `hive.invites(code, created_by, max_uses, uses, expires_at, on_ramp_hint)` are minted by existing members (quota per member, configurable) or by admins in bulk for Office Hours Global waves. Redeeming a code with a signed-in `profiles` row creates `hive.members` with `status = 'invited'`; completing one on-ramp flips it to `active`. All three on-ramps must be fully self-service on day one (Q16).
4. **Three on-ramps, one status (D20).** `on_ramp in ('purchase','compute','regional_server')` records how the member first qualified; later on-ramps append to `hive.member_onramps`. Any active member may later add any other on-ramp.
5. **Purchase on-ramp: Stripe with hub as reseller (D18, Q6).** A Stripe Checkout session is created by an Edge Function; the `checkout.session.completed` webhook (Edge Function, verified signature) writes a `purchase` ledger entry crediting the member's wallet in $honey (ADR-002) and activates membership. Provider keys (Anthropic, OpenAI, Nous, …) are stored in Supabase Vault and read only by the hub-side provider adapters (ADR-005); no member ever brings or sees a provider key in v1.
6. **Compute on-ramp: node registration.** The node app signs in, accepts the contributor ToS (decision 10), sets `allow_internet` and `tools_level` (ADR-006), runs a capability probe (ADR-003), and calls an Edge Function that inserts `hive.nodes` and activates membership. Nodes then receive short-lived hub tokens from the coordinator, not a raw Supabase write JWT (ADR-005).
7. **Regional-server on-ramp: server registration.** `hive-server register` performs the same sign-in via device-code flow, accepts the ToS, registers `region`, `storage_gb_offered`, `bandwidth_mbps` into `hive.regional_servers` (ADR-004, ADR-007), and activates membership.
8. **Project roles in `hive.project_roles` (D6).** `hive.project_roles(project_id, profile_id, role in ('owner','admin','follower'), granted_by, created_at)` with exactly one `owner` per project (partial unique index). Owner is set by trigger at project creation (the interviewing member). Owners invite Admins; Admins and Owners accept or reject Follower suggestions (`hive.card_suggestions`). Following a project is self-service for any active member.
9. **Read-all-for-members RLS (D8, D35).** Every `hive.*` table that holds project data has a `select` policy `hive.is_active_member()`; write policies use `hive.is_project_admin(project_id)` (owner or admin) or, for `hive.card_suggestions` inserts, `hive.is_active_member()`. Wallet and ledger rows are readable only by their owner and hub roles. The `anon` role has no grants on schema `hive`; nothing is publicly readable.
10. **Contributor ToS at node/server registration (Q15).** Registration cannot complete until the member accepts a versioned ToS stating: you provide compute or storage; you earn $honey (closed-loop, no cash-out, D67); you acquire no rights in any output (D53); you agree not to redistribute `owner_only` material you can see (D55); you accept that sandboxed member tasks run on your machine under the policy you set (ADR-006). `hive.members.tos_accepted_at` and `tos_version` record acceptance; a new ToS version re-prompts before the next check-in.
11. **Membership gates the overlay too.** Regional servers and nodes verify a member's hub token (minted by the coordinator after checking `hive.members.status = 'active'`) before serving artifacts or accepting control traffic, so D8's "nothing outside" holds at the network edge, not only at the DB.

Schema sketch (schema `hive`):

```sql
create table hive.members (
  profile_id uuid primary key references public.profiles(id) on delete cascade,
  status text not null default 'invited' check (status in ('invited','active','suspended')),
  on_ramp text check (on_ramp in ('purchase','compute','regional_server')),
  invited_by uuid references public.profiles(id),
  invite_code_id uuid references hive.invites(id),
  wallet_id uuid not null references hive.wallets(id),
  tos_version text, tos_accepted_at timestamptz,
  created_at timestamptz not null default now()
);
create table hive.project_roles (
  project_id uuid references hive.projects(id) on delete cascade,
  profile_id uuid references public.profiles(id) on delete cascade,
  role text not null check (role in ('owner','admin','follower')),
  granted_by uuid references public.profiles(id),
  created_at timestamptz not null default now(),
  primary key (project_id, profile_id)
);
create unique index one_owner_per_project on hive.project_roles(project_id) where role = 'owner';

create function hive.is_active_member() returns boolean language sql security definer stable as $$
  select exists (select 1 from hive.members where profile_id = auth.uid() and status = 'active') $$;
create function hive.is_project_admin(pid uuid) returns boolean language sql security definer stable as $$
  select exists (select 1 from hive.project_roles
                 where project_id = pid and profile_id = auth.uid() and role in ('owner','admin')) $$;

alter table hive.projects enable row level security;
create policy members_read on hive.projects for select using (hive.is_active_member());
create policy admins_write on hive.projects for update using (hive.is_project_admin(id));
```

On-ramp flow (all three end in the same state):

```
sign in (Apple|Google) ─► profiles row ─► redeem invite ─► hive.members(status='invited')
     ├─ Stripe Checkout ─► webhook ─► ledger 'purchase' ─┐
     ├─ node app: ToS + policy + probe ─► hive.nodes ────┼─► status='active', wallet live
     └─ hive-server register: ToS + capacity ─► hive.regional_servers ─┘
```

## Consequences

### Positive
- Zero new identity surface: no passwords to store, no second account for Cmd Work users, and Apple/Google handle MFA and recovery.
- Membership is a single row and a single `is_active_member()` predicate, so D8's read-all rule is one policy per table and auditable.
- Keeping `hive.*` in its own schema leaves Cmd Work's `public` grants, RLS and Realtime untouched (D32).
- Hub-as-reseller keeps provider keys in one Vault and lets $honey be priced from a single cost table (ADR-002).
- ToS at registration is enforced by the same code path on every on-ramp, so the licensing rules in ADR-011 have a legal footing before the first card runs.

### Negative
- Apple-only and Google-only sign-in excludes members without either account; email magic links would widen the funnel but are not in scope.
- Web SIWA needs an Apple Services ID, a verified domain and a redirect URL; until that portal work is done, web sign-in is Google-only.
- Hub-as-reseller makes Happy Jack Media the merchant of record for API credit: Stripe fees, refunds and sales-tax handling land on the Hive.
- Read-all-for-members means a suspended member who retained a session could still read until the JWT expires; policies check `status` live, but overlay tokens must be short-lived too.

### Risks & mitigations
- **Invite-code leakage during a 2,000-member surge.** Mitigation: codes carry `max_uses` and `expires_at`; admin bulk codes are single-use; redemption is rate-limited per IP and per profile.
- **Stripe webhook replay or forgery.** Mitigation: signature verification, idempotency on `stripe_event_id`, and ledger writes only from the webhook Edge Function with the service role.
- **Provider key exposure.** Mitigation: keys exist only in Supabase Vault, read by hub adapters; no client, node or regional server ever receives them; rotate on any suspected leak.
- **RLS drift between `public` and `hive`.** Mitigation: `hive` has its own helper functions and a migration test that asserts `anon` has no grants and every `hive` table has RLS enabled.
- **ToS not legally reviewed before launch.** Mitigation: flagged as a launch blocker (Q15); ToS text is versioned so it can be updated and re-accepted without a schema change.

## Open questions
- Is hub-as-reseller confirmed over bring-your-own-key? Default assumption: hub is reseller (D18 implication, Q6); BYOK deferred.
- Do invites expire, and how many may an ordinary member mint? Default assumption: 30-day expiry, 5 active codes per member, unlimited for admins.
- Should `follower` be an explicit role row or implicit for any member who views a project? Default assumption: explicit row, so suggestions and notifications have a subject.
- Can a suspended member's nodes keep earning until their leases finish? Default assumption: live leases finish, no new leases, wallet frozen pending review.
- Is `owner` transferable? Default assumption: yes, by the current owner only, in a single transaction that keeps the one-owner invariant.
- What does the ToS say about data retention of `owner_only` material cached on a contributor's node? Open — ADR-011; default assumption: scratch directories are wiped at lease release.

## Related
- ADR-001-hub-and-source-of-record
- ADR-002-honey-economics
- ADR-003-node-core-and-backends
- ADR-004-p2p-overlay-and-regional-servers
- ADR-005-scheduler-and-leases
- ADR-006-agent-runtime-and-sandbox
- ADR-007-artifact-storage
- ADR-009-web-app
- ADR-010-node-desktop-app
- ADR-011-ownership-and-licensing
- ADR-012-scope-and-roadmap
