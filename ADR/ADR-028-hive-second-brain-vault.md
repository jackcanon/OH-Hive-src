# ADR-028: A Second-Brain Knowledge Vault Built Into Hive

**Status:** Proposed · **Date:** 2026-09-14 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** Jack: "I want to make sure that we are integrating a second brain like system into Hive so that our agents are able to search data libraries that we curate during production. Something similar to an obsidian vault, but it's built into Hive."

## Context

Two things this ADR needs Jack to settle before a real design commits to one shape, because each answer points somewhere genuinely different:

**Whose library, and about what?** "Data libraries we curate during production" could mean a member's own personal knowledge base (research notes, decisions, reference material — a private Obsidian-style vault any Hive member keeps, that member's own agents can search), or it could mean something specific to Happy Jack Media's own content production (show notes, research, past episodes) that this project's agents should be able to draw on when doing Happy Jack Media work specifically. Those aren't the same feature — one is a general per-member Hive capability, the other is a Jack-specific curated corpus. This ADR is written assuming the general, per-member-vault reading (it's the one that's "built into Hive" as a platform capability, matching how everything else in this project's connector/skill/fleet work has been scoped as a Hive feature rather than a one-off), but **flagged for Jack to confirm or correct** rather than assumed silently.

**Build a new note-taking surface, or index an existing Obsidian vault?** Obsidian itself is just a folder of markdown files with `[[wikilink]]`-style backlinks — no proprietary format, no server. The fastest real version of "something similar to an Obsidian vault, but built into Hive" isn't writing a new markdown editor; it's pointing Hive's indexer at a folder a member already has (an actual existing Obsidian vault, or any folder of markdown notes) and making its contents searchable by that member's own agents. A Hive-native editing surface is a much bigger, separate UI investment that can come later if members want one without needing a separate Obsidian install.

## Decision

### 1. Scope: a per-member vault, not a shared community library

Same trust posture as everything else this project defaults to (BYOK, Private Fleet, local-first connectors): a member's vault is their own curated folder, searchable by their own agents, not automatically pooled into anything shared across Hive. A community-shared knowledge base (everyone's curated notes, searchable by everyone's agents) is a real and different feature — explicitly not this ADR, flagged as an open question below rather than built by default.

### 2. v1 storage: index an existing folder of markdown files, don't build a new editor

A member points Hive at a directory on their machine — their actual Obsidian vault if they have one, or any folder of `.md` files otherwise. Hive treats it as read-mostly external state: index it, watch it for changes, don't require the member to switch note-taking tools. No new proprietary format, no migration. A Hive-native note editor (so a member never needs Obsidian itself installed) is a real fast-follow, not v1 — same "prove the shape with the smaller version first" sequencing this project has used repeatedly (ADR-022's "web first," ADR-026's "create-only Drive before a real picker").

### 3. Indexing and search: local SQLite FTS5, matching ADR-025's precedent

Local-first, same as the fully-local hub direction: a lightweight indexer walks the vault directory, extracts each note's text + wikilinks + tags, and builds a full-text search index in SQLite (FTS5) — no new server, no cloud dependency, works fully offline. Re-index on file-change (a filesystem watcher) rather than on a timer, so search reflects the vault's actual current state. Semantic/embedding-based search (finding a conceptually related note that doesn't share exact keywords) is a natural v2 once full-text search proves the shape is useful at all — worth flagging now that it would need a local embedding model or an opt-in cloud call, not decided here.

### 4. Agent access: two new local tools, same family as ADR-024's file tools

`vault_search(query) -> [{path, title, snippet, score}]` and `vault_read(path) -> content`, added to the existing private-fleet coding-agent tool set (`read_file`/`write_file`/`list_dir`/`run_command`) and, separately, as tools the on-device `ChatEngine`/`FeedbackAssistant` sessions could also get (same "connecting an account and a model acting on it are separable decisions" framing ADR-026 already used for Drive/Gmail tool-calling access — not solved here, just noted as the same shape of follow-on decision).

### 5. Curation stays a human act — v1 agents read, they don't write to the vault

A "curated" library implies deliberate editorial judgment about what belongs in it. v1 gives agents search + read only; whether an agent should ever be allowed to file its own notes into the vault (a natural pairing with ADR-027's self-improving skills — "the agent learned something, save it somewhere") is a real future question but conflating "the member's carefully curated reference material" with "things the agent decided to jot down" in the same store, on day one, risks the vault filling with agent-generated noise before it's proven useful as curated input. Keep them separate stores until there's a real reason to merge them.

## Consequences

**Positive:** genuinely fast v1 — no new note-taking UI, no new content format, works with a tool members may already use. Local-first matches this project's whole privacy posture; a member's notes never leave their machine unless they choose to sync the underlying folder themselves (iCloud, Obsidian Sync, git — Hive doesn't need an opinion on that). Gives agents (coding agent, chat, feedback assistant) a genuinely useful new capability — grounded answers from a member's own reference material instead of only training-data knowledge.

**Negative / risks:** "index an existing vault" means Hive inherits whatever mess is already in that folder — stale notes, contradictory information, nothing enforcing quality, since curation is entirely the member's own responsibility (arguably a feature, not a bug, matching Obsidian's own philosophy, but worth naming). FTS5 keyword search will miss conceptually-related notes that don't share vocabulary — a real limitation until/unless semantic search is added. A filesystem watcher adds a small always-running background cost per paired machine.

**Open questions, not decided here:**
- Confirm scope (Decision context, question 1): general per-member Hive feature, or something narrower/Happy-Jack-Media-specific?
- Should a community-shared version of this (opt-in, member-curated, searchable across the community) ever exist, and if so what's the ownership/moderation model — this is a materially bigger question than the per-member version and shouldn't be assumed as a natural extension without its own look.
- Does a connected vault sync across a member's own paired Macs (same open question ADR-026 and ADR-027 both already carry for their own local-first stores) — worth solving once, for all three, rather than three separate one-off answers.
- Semantic search: local embedding model vs. opt-in cloud call vs. FTS5-only indefinitely.
- Whether/how ADR-027's self-improving skills and this vault ever connect (a skill citing or filing into a vault note) — deliberately kept as two separate stores in v1 per Decision 5.

## Amendment (2026-09-14) — scope confirmed and corrected: user-level, whole Private Fleet, not one machine

Jack confirmed the general reading (Decision context, question 1) but corrected an assumption this
draft had baked into Decisions 2–3 without saying so: "It's not meant to be used by the overall
Hive, it's just supposed to be local to each user, not necessarily each node... we want it to be
used at the user level, not the whole Hive, and not limited to just one computer. It's the Private
Fleet." Also: "I want it to be a part of the local desktop app (Swift to start and Tauri to
follow)."

Two real corrections to the v1 design above:

**Not single-machine.** Decisions 2–3 as written ("a member points Hive at a directory on their
machine," a local SQLite index on that one machine) describe a single-computer vault. That's wrong
for what Jack asked for: the vault belongs to the *member*, reachable from every machine in that
member's own Private Fleet, not pinned to whichever Mac happens to hold the folder. This is exactly
the shape ADR-025 already solved for a different kind of state — one designated machine in the
fleet runs the actual store (`LocalHub`'s SQLite + a small LAN/Cloudflare-Tunnel-reachable server,
reusing the same pairing and reachability machinery already built for cards/projects/checkpoints),
and the member's other paired machines reach it the same way they already reach that fleet's
`LocalHub` for everything else. The vault index almost certainly belongs as more tables inside the
same `LocalHub` SQLite store, not a second, separate local database with its own sync story — one
mechanism for "this member's own data, everywhere in their fleet, never through Supabase" rather
than reinventing it per feature. Still explicitly not community-shared, not node-scoped: user-level,
fleet-wide, exactly the boundary ADR-025 already drew for private-fleet project data.

**Not Swift-only.** "Swift to start and Tauri to follow" means the indexing/search/sync logic
belongs in the shared Rust core (`ohhive-core`, `LocalHub` territory) exposed via UniFFI, the same
pattern already used for chat memory, kanban, and local-hub pairing — a Swift UI first, a Tauri
(JS) UI reusing the identical Rust core later, rather than writing this feature twice. This is a
different shape from the Google connector work done the same day (deliberately pure-Swift,
single-machine, zero Hive-core involvement, per ADR-026's own "Swift app only" scope) — the vault
is closer in shape to `LocalHub` than to a Google OAuth token.

This changes Decision 2 and 3 above from "the fastest true v1" to "the fastest v1 *once the
LocalHub-integration question is actually checked*" — ADR-025 was written for structured project/
card/checkpoint rows, not arbitrary markdown files with a filesystem watcher; whether that same
SQLite store and sync path extends cleanly to "watch a folder, keep every paired machine's index
current" needs to be verified against `LocalHub`'s actual implementation before committing to it,
rather than assumed by analogy. Queuing that specific check to Sif (she owns `LocalHub`'s build,
per CONTINUITY.md) rather than guessing at it here.

## Amendment (2026-09-14) — Sif's LocalHub review: correcting an overstatement, and the real fork

Sif's review (`docs/SIF-VAULT-LOCALHUB-REVIEW-2026-09-14.md`) confirms the direction — reuse
`LocalHub`'s database, transport, and pairing identity for the vault — but corrects something the
amendment above overstated. It did not say "the vault almost certainly belongs as more tables
inside the same store, reachable the same way" as if that reach already exists; it said this is a
**clean extension requiring new work**, not a reuse of an existing sync path, because **no such
path exists yet**: `LocalHub` is one centrally-queried SQLite database on one designated machine,
not replicated state — every paired machine already reaches cards/projects/checkpoints the same
"ask the hub machine" way the vault would, but that was never "sync," it was "the hub machine is
the only place that data lives, everyone else asks it over the network." Also flagged plainly: no
`LocalHub` symbols exist in the current UniFFI Rust-to-Swift bridge at all, and the FFI crate's
feature flags omit `local-hub` entirely — "the ADR overstates existing desktop integration." Both
are useful corrections, not just caveats; recorded here rather than left standing uncorrected.

Concrete engineering findings, not previously known: `local_hub`'s schema is pinned to
`user_version=1` and rejects anything higher, so a real versioned migration is needed, not just
appended tables. Indexing must not run under the same immediate-transaction mutex leases/heartbeats
already contend for — file I/O, hashing, and parsing happen outside the database lock, in small
atomic batches. A filesystem watcher alone is not a robust indexer (editors save via different
rename/write sequences, some filesystems emit no events, a failed or partial scan must never be
read as "everything else was deleted") — needs an initial scan plus periodic reconciliation, not
events-only. Note content must be treated as untrusted task data an agent reads, never as
instructions that can grant tools or change permissions, when it's fed into any cloud-coordinated
session. Hub backups now contain private document text, not just pairing/project state — the
backup/export story needs to account for that. Revocation must apply to search, not just direct
reads.

**Decided (2026-09-14, Jack): true multi-machine sync** — notes must be addable/editable from any
machine in the fleet, even while others are offline, not just centrally hosted on one designated
machine. Overrides this document's own recommendation (central-query v1); accepted knowingly, not
by default — Sif's assessment stands that this is genuinely "a larger separate project" than the
vault's core indexing/search work, not a small extension of it. This does not block starting the
indexing/search/`VaultStore` work itself (still needed either way, per the build order below) — it
means the source-agent ingestion protocol Sif sketched (owner-authorized source per machine,
sequenced/acknowledged upsert-delete messages, durable outbox, manifest reconciliation, one
publisher per source namespace) is real v1 scope, not deferred follow-on work. Queued to Sif via
CONTINUITY.md: turn that sketch into an actual design proposal — stable file identity across
machines, conflict handling, delete propagation, interrupted-transfer/retry behavior, and a realistic
complexity/time estimate — before any of it gets built, same "propose, don't build yet" process
used for the Nango hosting question.

~~Suggested build order once that's answered... remote source-folder ingestion only if the
central-folder answer turns out to be insufficient in practice.~~ **Stale as of Sif's full sync
proposal below — multi-writer replicas are now the decided requirement, not a maybe-later
extension.** Left struck through rather than deleted so the reasoning trail stays intact.

## Amendment (2026-09-14) — Sif's full design proposal: real scope, real cost

Sif's design (`docs/SIF-VAULT-MULTIMACHINE-SYNC-PROPOSAL-2026-09-14.md`) — hub-mediated exchange
with full offline-capable replicas on every machine, not peer-to-peer, not central-query. One
correction to her own earlier ingestion sketch: "one publisher per source namespace" can't model
several machines editing the same note, so each device gets its own operation namespace while all
devices share the same document identities; the hub validates/stores/distributes operations and
each replica edits/searches independently of it. Immutable, content-addressed revisions with
explicit parent ancestry (no clock-based last-write-wins, no silent AI merge) — concurrent edits on
two offline machines are preserved as both variants and surfaced as a conflict for the member to
resolve by hand, never guessed at. Stable `document_id`s survive renames and don't live in note
frontmatter. Durable per-device outboxes, idempotent acknowledgement, and periodic manifest
reconciliation handle interrupted transfers and long-offline devices without silently inferring
mass deletes from an incomplete scan.

**The number that matters most: 12–20 engineer-weeks for a defensible Markdown-only v1** (roughly
8–12 elapsed weeks with two engineers, given real integration/hardware test gates that don't
parallelize cleanly) — explicitly a judgment-based planning estimate, not a bid. Attachments, a full
Tauri sync UI, automatic text merging, selective sync, peer-to-peer exchange, and automatic hub
failover are all named as excluded from that range, not included-but-unstated.

**Concrete decisions Sif is asking for before any of this is built** (her Section 11, not yet
answered): hub-mediated exchange vs. peer-to-peer (she recommends hub-mediated; stated as a decision
to confirm, not assumed); Markdown-only initial scope vs. expanding to attachments at launch (a real
scope/cost change, not a detail); retention/quota defaults for deleted-note recovery; how much
conflict-resolution UI belongs in the very first shell that ships. Sent to Jack alongside the cost
number, since the honest price of "true multi-machine sync" is new information that could
reasonably change which of these get answered which way — including whether to build the full
version now at all versus starting from her originally-recommended simpler central-query v1 and
returning to full sync later.

`VaultStore`/indexing/search can start independently of how the sync question lands, but must use
`document_id + revision` as the identity model from day one (not a file path, not one shared index
copy assumed fleet-wide) so it isn't rebuilt if/when sync work follows.

## Amendment (2026-09-14) — decided: start with central-query v1, full sync stays designed-not-built

Given the real 12–20 engineer-week cost, and everything else already in flight (ADR-024's cloud
brain, the Swift UI redesign, Google Workspace), **Jack chose to start with Sif's originally
recommended simpler v1** — one designated fleet machine holds the vault folder, every paired
machine's agent can search/read it, editing happens on that machine (vault reads honestly fail as
"unavailable" rather than silently stale if it's offline) — rather than commit to the full
multi-writer sync design now. Full sync is not rejected, just not started: both this ADR and Sif's
complete sync proposal stay as a real, already-designed option to pick up later if the simpler
version proves the vault is used enough to justify the bigger build.

Build now proceeds on Sif's earlier central-query build order (versioned schema migration →
`VaultStore` with `vault_list`/`vault_status`/`vault_search`/`vault_read` → watcher + recovery-scan
→ authenticated grants + two-machine test → UniFFI wiring to Swift, then Tauri) — with one carryover
from the sync design kept regardless, per both proposals' agreement: identity is `document_id +
revision`, never a bare file path, so the central-query v1 doesn't have to be rebuilt if/when full
sync work follows later. Queued to Sif via CONTINUITY.md to start this build.

## Amendment (2026-09-14) — first UniFFI/Swift wiring: this machine only, notes by hand

Sif's build order's last step ("UniFFI wiring to Swift, then Tauri") was still fully open going
into this — her own build report flagged **zero** existing `LocalHub`/vault symbols anywhere in
the Rust-to-Swift bridge. Built while she works the computer-use proposal (ADR-029) in parallel,
per Jack's "what can you move forward while she's doing that" — the storage/search/HTTP-reader
layer was already built and independently code-reviewed (both her core-storage and
folder-ingestion milestones; see CONTINUITY.md), so this was ready to wire without waiting on her.

**What's now real, pending Jack's build/verify:** `crates/ohhive-ffi/src/local_hub.rs` (new) adds
`vault_open`/`vault_create`/`vault_list`/`vault_search`/`vault_read`/`vault_add_note`/
`vault_remove_note` to `HiveNode`, and a "Vault" row under the Swift app's Private Fleet sidebar
section (`VaultView.swift`) — create a vault, add/edit/delete Markdown notes by hand, search, read.
Two small, additive core changes support it: `LocalHubStore::vault_list_all` (owner-side inventory
across every vault, for the "manage my vaults" screen — nothing like it existed; every prior
listing was reader-grant-scoped) and `LocalHub::node_id` (lets a session read back its own device
id, so a freshly created vault can self-grant without the owner pasting their own machine's id back
to itself). `ohhive-ffi`'s `hive-core` dependency now always builds with the `local-hub` feature
(previously entirely absent, matching Sif's "feature flags omit local-hub entirely" finding).

**Deliberately not in this pass, same "prove the shape with the smaller version first" sequencing
this ADR has used throughout:**
- **Folder ingestion isn't wired.** `vault_folder.rs`'s watcher/reconciliation (Sif's second
  milestone, also independently reviewed and confirmed sound) has no Swift entry point yet — v1
  is hand-added notes only. A real, scoped next increment, not forgotten.
- **Reading from a *second* Private Fleet machine isn't wired.** This pass's `LocalHub` reader is
  strictly in-process (this machine, talking to its own local SQLite file) — it never calls
  `local_hub::transport::serve()` or `RemoteLocalHub`. Central-query v1 as decided above means one
  designated machine hosts the vault and others query it remotely; that HTTP host/pair/grant path
  already exists and is tested (Sif's report), but wiring it into Swift (start/stop the host
  listener, mint and redeem a pairing code, and — since "pairing alone does not grant access" is
  load-bearing, not a gap — a real UI step for the owner to grant a specific paired machine's
  device id) is real, undone follow-on work. `local_hub.rs`'s reader is written as a thin seam
  precisely so that follow-on swaps in a `RemoteLocalHub` branch rather than being a rewrite.
- No agent tool wiring yet (Decision 4's `vault_search`/`vault_read` as coding-agent/chat tools) —
  this pass is the human-facing UI only.

**Real bug found in Jack's first live test, fixed same day:** `from_connection` marks every vault
`unavailable` on store-open, by design, so a folder-backed vault never shows stale content before
its watcher re-scans — but a hand-curated vault has no watcher to ever undo that, so it went
permanently `unavailable` (search failing with "hub unreachable: vault unavailable") the very first
time the app relaunched after creating one. Not a bug in the core's documented behavior; a gap in
this pass's integration of it. Fixed with `LocalHubStore::vault_reopen_manual` (republishes any
vault with no `vault_sources` row — i.e. not folder-backed — back to ready on open; a real
folder-backed vault is deliberately left alone, since its own reconciliation is what's allowed to
republish it), called once per real store-open from `vault_open()`. Also fixed, same session: the
note editor's Save button silently disabled itself when the path field didn't end in `.md`, with no
explanation — now the app appends `.md` automatically (or slugs the title if the path is left
blank) rather than requiring the member to know that rule.

## Amendment (2026-09-14) — agent-tool wiring: vault_search/vault_read added to the coding agent

Closes the last of the three gaps the previous amendment listed as deliberately not done yet —
Decision 4's `vault_search`/`vault_read` as coding-agent tools. `crates/ohhive-core/src/coder.rs`
(Loki-owned, ADR-024/ADR-025 territory) gains `CodeSessionSpec.vault_name: Option<String>`
(host-trusted card data set when a `code` card is created, same as `workspace_path`/`task` — never
something the running brain can choose or widen) and two new tools, always advertised in
`tool_specs()` so a brain can discover them without a round-trip: calling either one without a
configured `vault_name` is a clean tool error, not a panic. Reuses the exact same "this machine
only" vault store the desktop app already reads (`ohhive-ffi/src/local_hub.rs`'s
`vault-host.sqlite3`, same `HIVE_VAULT_SELF_KEY` open/enroll dance) rather than inventing a second
path, resolved fresh on every tool call (no cached reader handle), same as every other tool in that
file. Feature-gated on `local-hub` independently of the `sandbox`/`hub` gate `coder.rs` itself needs
to compile at all — see CONTINUITY.md for why that distinction matters here and for this session's
verification status (not yet compiler-checked; a real build/test run is queued to Jack).

**Still open:** cross-machine reading and folder ingestion — unchanged by this amendment, still
Swift/FFI work tracked as before. Of the three gaps the prior amendment listed, agent tools are now
done (this amendment); the other two remain.

## Amendment (2026-09-14) — decided: Hive gets its own editor and human/agent joint curation

Jack's call, in response to Sif's three-part vault-improvement review (curation-pipeline design,
SiYuan/Trilium comparison, open-source second-brain survey) and today's Obsidian licensing
research: Hive builds its own documenting experience rather than staying "index an existing folder,
agents read-only." This supersedes Decision 2 ("v1 storage: index an existing folder of markdown
files, don't build a new editor") and Decision 5 ("curation stays a human act — v1 agents read,
they don't write to the vault") as the eventual destination — both stay true for the increments
already shipped and in flight (folder-watching, the desktop hand-editor, `vault_search`/
`vault_read`), which are not being ripped out, just no longer the final shape.

No third-party app (SiYuan, Trilium) is being adopted as the underlying store — Sif's comparison
found both manage their own proprietary formats requiring an adapter layer, and neither is
validated for concurrent human+agent access. Hive's own Markdown-folder-plus-SQLite-FTS5 foundation
stays the storage layer; what's changing is who's allowed to write to it and what sits on top of it.

**Phasing, so this doesn't become one giant undesigned build:**

1. **Managed intake + auto-filing + source-grounded summaries** (Sif's own suggested starting
   point, reusing the existing Rust vault foundations — `local_hub/vault*.rs`) — no agent write
   access to curated content yet, no new UI, no semantic retrieval yet. Bounded and reversible:
   filing/tagging/summarizing/dedup only, per Sif's proposal. **Queued to Sif next** (see
   CONTINUITY.md).
2. **Agent write-access policy** — the genuinely unsettled, risk-bearing piece Sif's proposal
   flagged explicitly: what class of change an agent can make unreviewed versus what needs a human
   pass, how revisions stay undoable, how this can never become a path to expanding what an agent is
   otherwise allowed to see or do. Gets its own design pass (likely its own ADR or a substantial
   amendment here) before any code — not being improvised inline.
3. **The native editor itself** (replacing "point Hive at an existing folder" with a real authoring
   surface) — sequenced after 1 and 2 are further along, since building an editor for a store whose
   write-access model isn't decided yet is backwards.

Not starting phase 3 today. Phase 1 is Sif's next queued item; phase 2 needs Jack + Loki design work
together before it's anyone's to build.

## Related

ADR-025 (fully local hub — the SQLite/local-first precedent this reuses), ADR-024 (coding agent tool set — where `vault_search`/`vault_read` join), ADR-026 (personal connectors — the "read/search access and an agent acting on it are separable decisions" framing, and the per-machine-vs-synced question this ADR shares), ADR-027 (self-improving skills — a related but deliberately separate store, see Decision 5).
