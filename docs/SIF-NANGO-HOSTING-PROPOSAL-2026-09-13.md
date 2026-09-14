# Nango hosting proposal for task #204

**Sif your friendly Codex Agent — 2026-09-13. Proposal only; nothing deployed or purchased.**

Recommend a dedicated, Hive-operated connector service with its own Postgres instance, initially
using Nango for authentication/proxying and a small number of Hive-owned action adapters. Keep the
regional volunteer/control servers outside this credential boundary. Google Drive/Gmail v1 remains
Claude's separate local OAuth implementation; this proposal covers providers three onward.

## Correct the product assumption first

Free self-hosting is an Auth/Proxy foundation, not the packaged agent tools/sync/MCP offering.
The latter needs the paid edition. Jack requested that this distinction be reported to Claude;
it is already flagged in continuity. [Official feature comparison](https://nango.dev/docs/guides/platform/self-hosting#feature-availability)

A provider connection lets us authenticate API requests. A useful agent action still needs a
schema, ownership checks, provider-specific implementation, permissions, errors and user consent.
For example, “connect GitHub” and “create an issue with the approved title/body” are separate pieces.
Before 1.0, promise a tested provider/action list, not a catalog-wide capability claim. An existing
provider MCP implementation may supply actions later, but then it needs its own compatibility,
credential-handling and permission review; Nango Auth alone does not establish that integration.

The repository's root license is ELv2, including a restriction on exposing a substantial portion
of the software as a hosted/managed service. This is not a legal determination that embedding it
in Hive is forbidden. Confirm the selected version/component licenses and Hive's customer-facing
use with Nango before production, rather than treating “self-hosted” as an unconditional license
to offer a generic connector brokerage service. Do not remove licensed feature gates.
[Current repository license](https://raw.githubusercontent.com/NangoHQ/nango/master/LICENSE)

## What the fleet actually shows

Read-only snapshot: 2026-09-14 04:50 UTC (September 13 Phoenix). All three regional servers were
online with zero reported connections. Chicago is standby; Amsterdam and Sydney are primary.
Chicago SSH measured 3,915 MiB total memory, 3,352 MiB available, 0.04 short-term load average and
68 GiB free disk. That is spare capacity at one instant, not a reservation or a load test.
Amsterdam/Sydney memory and CPU headroom were not independently measured; their registry hardware
fields are empty. I would not infer their suitability from zero reported connections.
Evidence: `docs/nango-research/2026-09-13/`.

| Option | Assessment |
|---|---|
| Existing Chicago + separate database/cache | Lowest incremental app cost and reasonable for a disposable feasibility test. Shares host administration, restart risk and outbound network access with Hive. Not my production recommendation for member credentials. |
| Existing Amsterdam or Sydney | Same shared-host drawbacks; both currently primary. No verified resource headroom. Only reconsider for a clear residency/latency requirement. |
| New dedicated connector VM + separate Postgres/cache | Recommended. Independent patching, credentials, backup/restore and firewall policy. Start in a suitable US region near the database; verify availability before provisioning. |
| Enterprise self-hosted Nango | Alternative if ready-made tools/syncs are required. Obtain a commercial quote and vendor sizing; do not fit its larger service architecture into the small Auth/Proxy budget. |

## Rough monthly cost

Planning assumptions for a small Auth/Proxy deployment, not vendor-certified sizing. Published
North America prices show a 4 GB shared VM at $24/month, a 2 GB VM at $12, and single-node managed
Postgres at $16 for 1 GB or $32 for 2 GB. Region availability, taxes and actual account pricing must
be checked when provisioning. [Akamai published pricing](https://www.akamai.com/cloud/pricing/north-america)

| Layout | Base infrastructure | Planning allowance | Estimated total |
|---|---:|---:|---:|
| Recommended: new 4 GB app VM + 2 GB managed Postgres + separate 2 GB Redis/Valkey VM | $24 + $32 + $12 = $68 | $10–20 backup/monitoring/overhead reserve | **$78–88/month** |
| Smaller initial database: same layout with 1 GB managed Postgres | $52 | $10–20 | $62–72/month |
| Reuse Chicago app host; dedicated 2 GB managed Postgres and cache VM | $44 incremental | $10–20 | $54–64/month incremental |
| Operator-managed: new 4 GB app VM + separate 2 GB VM hosting Postgres/cache | $36 | $10–20 | $46–56/month, plus greater operating effort |

The reserve is my estimate, not a provider quote. These are single-instance designs, not high
availability. Exclude engineering labor, paid Nango licensing, provider charges and unusual egress.
The managed database recommendation buys maintenance separation; the self-managed variant is
cheaper but makes us responsible for upgrades, backup jobs, restore drills and incident response.
Do not buy anything yet. Pin a tested Nango release/digest and benchmark login, refresh and proxy
load before fixing production sizes. The repository's Compose file demonstrates separate server,
Postgres and Redis services, but should not be deployed verbatim as a production configuration.
[Upstream Compose](https://raw.githubusercontent.com/NangoHQ/nango/master/docker-compose.yaml)

## Credential store and operating design

Use a dedicated Postgres instance, not a schema in the existing Supabase Hive project. Give Nango
its own database/login and no network access to Hive's database. Restrict database/cache ingress to
the connector service; use verified TLS and direct PostgreSQL connections. Keep the operator
console behind private administrative access. The app-facing broker exposes only defined account
connection and action endpoints, not unrestricted Nango administration or arbitrary proxy URLs.

Supply a privately backed-up `NANGO_ENCRYPTION_KEY` before the first credential is written; fail
our deployment check if it is absent. Do not assume self-hosted storage inherits Nango Cloud's KMS
or disk encryption. The current self-hosting guide warns against simply changing this key after
setup. Store its recovery copy separately from database backups and prove decryption after restore.
[Self-hosting configuration](https://nango.dev/docs/guides/platform/self-hosting)

Protect the full database volume and backups as well as application-encrypted credentials: Nango
separately classifies connection metadata/configuration from encrypted credentials, and deletion
has a retention window. Set an explicit deletion/backup-retention policy and show honest disconnect
behavior rather than promising instant removal from every backup.
[Security/data lifecycle](https://nango.dev/docs/guides/platform/security)

Start with daily encrypted backups and a proposed 24-hour recovery-point / four-hour recovery-time
target, then test it. These are proposed targets, not guarantees. Disable outbound proxy URL
overrides, restrict provider destinations, and deny metadata/private addresses except the exact
internal database/cache destinations at the network layer. Run no community cards or user-supplied
integration code on this host. Encrypting tokens protects a stolen backup; it does not make a
compromised live broker unable to use them. Tell members this is Hive-operated cloud credential
custody, unlike Google v1's local Keychain custody.

## Swift connection flow

1. Swift sends its authenticated member session and selected provider to a narrow Hive connector
   broker. The broker derives member/community identity from verified authentication; it does not
   accept an arbitrary user ID or connection ID from the desktop.
2. The broker creates a Nango connect session using a server-only API key. Set the single intended
   `allowed_integrations` entry and server-generated ownership tags. Current API docs use `tags`;
   older `end_user`/`organization` fields are deprecated. Response includes `connect_link`, token
   and expiry. [Connect-session API](https://nango.dev/docs/reference/backend/http-api/connect/sessions/create)
3. Swift opens a Hive-hosted launch page in the system browser/authentication session. That page
   uses Nango Connect UI for the provider consent flow. Provider OAuth returns to Nango's registered
   HTTPS callback; a separate short-lived, single-use Hive completion state returns to the app.
   Prototype that handoff before choosing a custom scheme versus an associated HTTPS callback.
   Do not put provider login inside a raw embedded WebView.
4. The broker verifies connection completion server-side and binds it to the pending member/session.
   With free Auth/Proxy, plan polling of connection status rather than depending on a paid webhook
   feature. A browser callback or guessed connection ID alone is not authorization.
5. Store only connection identity/status in the app. Keep Nango environment keys and provider refresh
   tokens off the desktop. Later action requests pass through ownership checks and a provider/action
   allowlist; sending messages or changing external content needs the separately designed approval
   UX. None of these connection steps gives community workers access to personal accounts.
6. Disconnect denies further broker calls immediately, removes the Nango connection and, where
   supported, revokes the provider grant. The UI should expose which account and scopes are connected.

Suggested app abstraction: one `ConnectorAccount` display model, with `localOAuth` (Google v1) and
`hostedBroker` credential backends. That keeps a consistent Settings experience without pretending
both storage models have the same privacy properties. Fleet-only users should not be forced to
join a community merely to retain existing local Google connectors. A future user-operated Nango
broker needs explicit configuration/trust; do not silently redirect personal credentials when a
user switches communities.

## Next decision for Jack and Claude

First settle the product contract: Auth/Proxy plus a small explicit Hive action catalog, or obtain
a paid self-hosted quote for the packaged-tool requirement. Then approve hosting budget, region,
license/use confirmation and credential custody. Only after that, build a disposable pilot with
synthetic accounts and prove connect, refresh, ownership isolation, revoke/disconnect and restore.
No Nango deployment is requested by this report; it is ready for review alongside ADR-026.
