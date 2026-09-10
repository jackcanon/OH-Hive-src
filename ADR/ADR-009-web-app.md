# ADR-009: Web App (ohghive.com)

**Status:** Proposed · **Date:** 2026-09-04 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** ADR-000 Q8–Q11, Q13–Q14, Q16–Q17, Q19; D26, D30, D33, D37, D55, D59, D63, D65, D68

## Context

Hive has three member-facing surfaces (D26): the web app, the node desktop app, and the headless regional server. The web app is where a member does everything that is not "run compute": start a project by talking to the interviewer agent, watch the kanban, manage the $honey wallet, and browse every other project in the Hive. It is the first thing a new invitee touches, and on day 1 it may have to absorb up to 2,000 sign-ups (D56) with zero manual steps.

The obvious shortcut — bolting the Hive onto Cmd Work — was rejected in Q9. Inspection of `CmdWork-src` showed Cmd Work is a native Swift app with no web front end yet, and its planned web stack is "Next.js or SvelteKit on Vercel". Jack's answer was a separate app that shares only the database and auth (D30). That keeps Cmd Work's grants, RLS and Realtime untouched, and lets the Hive live in its own Postgres schema `hive` (D32).

The web app is a thin client over hub state. The scheduler, the interviewer/planner, and all authority live on the hub (Supabase + the coordinator worker, ADR-001, ADR-005); nodes talk to the coordinator over the overlay, not through the browser (D59). The web app therefore has one job: render `hive.*` state faithfully and in real time, and let members act on it within their project role (D6, D35).

Because the mobile app is v1.1 rather than "someday" (D68), the web app is also the design source for mobile. It must be mobile-responsive from v1 and built on a React component package the Tauri app and the Expo wrapper can both consume (D33).

## Decision

1. **Separate codebase and deployment** (D30). The web app is its own repository/workspace and its own Vercel project. It shares nothing with Cmd Work except the Supabase project (DB + Auth). It never imports Cmd Work UI code and never reads `public.*` tables except `profiles`.
2. **Framework: Next.js on Vercel** (D33), using `supabase-js` with the Supabase Auth session (Apple + Google, D31). Web Sign in with Apple requires a separate Apple Services ID and redirect URL; that portal step is a launch checklist item, not code.
3. **Domain:** `ohghive.com` (D65, purchased 2026-09-04 via Vercel, team Happy Jack Media). Preview deployments stay on Vercel's default domains and are protected by Vercel deployment protection; no Hive data is reachable without a member session (D8).
4. **Data access is RLS-only.** The app uses the anon/publishable key plus the user JWT. There is no service-role key in the web tier. Every read of `hive.*` is gated by an active `hive.hive_members` row (D35); every write by `hive.project_roles` (owner/admin) or the follower `suggest` path.
5. **Surfaces shipped in v1:**
   - **Interview / chat-to-project** (D7, D37, D38). A chat view that streams interviewer turns from a hub Edge Function. On completion the plan materialises as `hive.projects` + `hive.cards` and the UI redirects to the new board. The wallet balance is shown inline because the interview is metered (D38).
   - **Kanban** (D34, D40–D42). Columns are a *view over the card DAG*: `Backlog` (unblocked deps pending), `Ready` (deps satisfied, no lease), `In Progress` (an active row in `hive.leases`), `Review`, `Done`. Cards show their modality, `required_capabilities`, dependency edges, current lease holder (node id, region) and last checkpoint time.
   - **Wallet** (D19). Balance derived from the append-only ledger; entry list by type (`purchase`, `earn_compute`, `earn_infra`, `fund_project`, `spend_job`); "fund project" action that transfers $honey to any project (D19). Purchase flow hands off to Stripe (ADR-002/008); no card data touches the app.
   - **Hive-wide project browser** (D8, D35). Read-all for members: every project, its license, cards, artifacts, and activity. No public/unauthenticated view (D67).
   - **Eligible-nodes indicator per card** (D63, Q13). Each card shows the count of currently checked-in nodes whose capability record satisfies `required_capabilities`, `requires_internet`, and `tools_level`. Zero eligible nodes renders a "why is this queued" explainer.
   - **Internet-required badge** (D47). Cards/projects with `requires_internet = true` show a badge; the eligible count only includes `allow_internet = true` nodes.
   - **Fork** (D55). Available only when `license = 'open_source'`; copies plan, cards and artifact pointers into a new project owned by the forker (ADR-011). Hidden, not merely disabled, for `owner_only`.
   - **Pending-return warnings** (D51). Banner on any project whose artifacts are in the storage grace period, with time remaining and a "fund project" shortcut.
6. **Realtime is UI-only** (D59). Supabase Realtime subscriptions drive kanban, lease, wallet and eligible-node updates in the browser. Node control traffic never goes through Realtime or the web app.
7. **Shared React component package** (D33). A workspace package (`@hive/ui`, name provisional) holds design tokens, kanban card, wallet widgets, capability badges, and chat primitives. The Tauri node app (ADR-010) consumes the same package; the v1.1 Expo app reuses the tokens and logic layers (D68).
8. **Mobile-responsive from v1** (D68). Every v1 route must be usable at 375 px width. Layout is designed mobile-first so v1.1 is a wrapper plus push notifications, not a rebuild.
9. **Artifact streaming — Open.** Members browse artifacts from the nearest regional server (D49). The gateway mechanism (regional server exposing HTTPS with member-scoped signed URLs, versus a signed-URL relay) is deferred to ADR-004/ADR-007. The web app codes against an `artifact_url(hash)` resolver so the choice is swappable.

### Data model touched by the web app (read/write summary)

| Table (`hive.`) | Web app reads | Web app writes |
|---|---|---|
| `hive_members` | own row, member list for presence | none (invite acceptance via Edge Function) |
| `projects`, `cards`, `project_roles` | all (D35) | owner/admin edits; follower `suggest` rows |
| `leases` | all, for "In Progress" and lease holder | none |
| `ledger` (append-only) | own entries + project funding totals | `fund_project` via RPC only |
| `nodes` / capability records | counts and summaries for eligibility | none (node app owns these, ADR-010) |
| `artifacts` | metadata, hash, replica locations, pin state | resubmit request (D52) via RPC |

## Consequences

### Positive
- Clean boundary with Cmd Work; Hive changes cannot break Cmd Work RLS or Realtime.
- RLS-only access means the web tier holds no secrets; a compromised Vercel deployment cannot read more than a member can.
- One component package feeds three shells (web, Tauri, Expo) — one design system, one bug fix.
- Rendering the kanban as a DAG view keeps the UI honest about what "In Progress" means (a live lease), which is what members need to understand when a node checks out.

### Negative
- Next.js + Vercel adds a third hosting bill and a second deploy pipeline next to Supabase and the regional servers.
- Every project-level derived number (eligible nodes, funding runway) is computed from `hive.*` on the client or in Postgres views; keeping those views fast at 2,000 nodes is hub work (ADR-005) the web app depends on.
- No public pages (D67) means no SEO/marketing surface from the app itself; a separate static landing page would be needed for invitations.

### Risks & mitigations
- **Realtime connection limits.** 2,000 concurrent browser sessions on Supabase Realtime is within Pro limits but not free. Mitigation: subscribe per-visible-project, not globally; fall back to polling on a 15 s interval if a channel fails.
- **Day-1 surge on invite acceptance and Stripe** (Q16). Mitigation: both are Edge Functions with idempotency keys; load-test at 2,000 sign-ups/hour before invites go out.
- **Artifact gateway undecided** blocks the project browser's media preview. Mitigation: ship v1 browser with metadata + download-on-node first if the gateway lags; the resolver abstraction (decision 9) means no UI rewrite.
- **Apple web sign-in setup** is a manual Apple Developer portal step; if missed, Apple-only members cannot log in on web. Mitigation: launch checklist item owned by Jack.

## Open questions
- How do members stream artifacts in the browser: regional server HTTPS gateway with member-scoped signed URLs, or a signed-URL relay through the hub? (Default assumption: regional server gateway; see ADR-004/ADR-007.)
- Does the follower `suggest` path create a real `hive.cards` row with `status='suggested'`, or a separate `hive.card_suggestions` table? (Default: separate table, promoted on accept.)
- Should the kanban expose the raw DAG (graph view) in v1, or only the column view? (Default: columns only; dependency edges shown on the card detail.)
- Does the interview chat run on Vercel streaming routes or entirely on Supabase Edge Functions? (Default: Edge Function owns the turn; Next.js only proxies the stream.)
- Should the web app support a "check in / check out" control for a member's own desktop nodes ahead of v1.1 mobile? (Default: yes, read-only status in v1; remote check-in is v1.1.)

## Related
- ADR-001-hub-and-source-of-record
- ADR-002-honey-economics
- ADR-004-p2p-overlay-and-regional-servers
- ADR-005-scheduler-and-leases
- ADR-007-artifact-storage
- ADR-008-auth-and-membership
- ADR-010-node-desktop-app
- ADR-011-ownership-and-licensing
- ADR-012-scope-and-roadmap
