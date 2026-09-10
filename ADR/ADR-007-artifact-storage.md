# ADR-007: Artifact Storage on Regional Servers

**Status:** Proposed · **Date:** 2026-09-04 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** ADR-000 Q3, Q12, Q14, Q17; D42, D49–D52

## Context

Hive projects produce media: video renders, images, audio stems, code bundles, plus the agent-loop checkpoints (D42) needed to resume cards across nodes. These payloads are large (video outputs and checkpoints in the hundreds of MB; image/video model weights 5–30 GB each, Q17) and they must never flow through Postgres (Q3). Supabase Storage was considered and rejected as the artifact store: the Cmd Work project is on Supabase Pro with a 250 GB/month egress cap (Q9), and a film/TV community streaming renders to every member (D8) would exhaust it in days.

Jack chose option B (Q14): **regional servers are the artifact store.** They already exist as the infra role (D21), run the same `hive` core headless (D27), are geo-registered (D58), and form the self-healing relay layer (D28). Storing artifacts on them reuses that footprint, keeps bytes near members, and gives server operators a way to earn $honey at a reduced rate (D50). The same infrastructure doubles as the model-weight cache so 2,000 nodes do not all pull from Hugging Face at once.

Storage is not free to the Hive, so it is funded: an artifact is pinned only while its project has $honey to cover it (D51). When funding runs out the artifact enters a grace period, is returned to the project owner, and is evicted. Owners can resubmit later and content addressing deduplicates the re-upload (D52). Postgres remains the source of record for *what exists and where*; the bytes live on the overlay.

## Decision

1. **Content-addressed artifacts on regional servers (D49).** Every artifact is identified by `blake3(content)`; large files are chunked (4 MiB) with a chunk manifest whose hash is the artifact id. Bytes are stored on regional servers under `<data_dir>/blobs/<hash[0:2]>/<hash>`. Nodes upload to the nearest online regional server via the overlay (ADR-004), which fans out replication.
2. **`hive.artifacts` holds metadata only (D49).** Postgres stores hash, size, MIME type, project/card provenance, licence inheritance, pin state and replica locations. No blob bytes, no chunks, no Supabase Storage objects for artifacts.
3. **No Supabase Storage for artifacts.** Supabase Storage may be used only for small, hub-owned objects (avatars, invite assets) where egress is negligible. Any table or code path writing artifact bytes to Supabase Storage is a defect.
4. **Replication factor 2 with re-replication.** Default `replication_factor = 2` on distinct regional servers, preferring the uploader's region for one replica and a different region for the second. The coordinator (ADR-005) runs a repair loop: when a server drops offline past its grace window, every artifact with fewer than 2 healthy replicas is re-replicated from a surviving copy. Servers below a minimum uptime score may not hold a sole replica.
5. **Funded pinning (D51).** Each project carries a storage meter: `bytes_pinned × seconds × storage_rate` is debited from the project's wallet on a daily settlement (ADR-002). When the wallet cannot cover the next period, artifacts move to `grace` for `grace_period` (default 14 days). During grace the owner's node app receives a **pending returns** inbox entry and the web app shows a banner.
6. **Return to owner, then evict (D51).** During grace the artifact is offered for download to the owner (node app auto-downloads if online; web app offers a gateway download). After the grace period, or once the owner has confirmed receipt, replicas are deleted and `hive.artifacts.state = 'returned'` with the hash retained for dedup.
7. **Resubmission and dedup (D52).** An owner re-uploading a returned artifact presents its manifest hash first; if the hash matches an existing `returned` row and the project is funded, the row flips to `pinned` and only missing chunks are transferred. Identical content across projects shares blobs but carries separate `hive.artifacts` rows (provenance and licence differ).
8. **Integrity checks and storage-earning penalties.** Every fetch verifies chunk hashes; regional servers run a background scrub (sample 1% of blobs/day). A server found serving corrupt or missing data for an artifact it claims to hold forfeits storage earnings for that settlement period, and the artifact is re-replicated elsewhere.
9. **Checkpoint payloads are artifacts (D42).** Agent-loop checkpoints (ADR-006) are stored through the same path with `kind = 'checkpoint'`, `replication_factor = 2`, and a short TTL: a checkpoint is auto-unpinned when the card finishes or when a newer checkpoint for the same card is written. They count toward the project's storage meter while live.
10. **Model-weight cache reuses the same infrastructure (Q17).** GGUF/safetensors weights from the model catalog are stored as `kind = 'model'` artifacts pinned by the Hive itself (not by a project), with `replication_factor` raised per region on demand. Nodes fetch weights from the nearest regional server before falling back to the upstream URL, and may seed to peers.
11. **Web-app gateway path stays open.** Members browsing projects (D8) fetch artifacts through a regional server exposing HTTPS (`GET /a/<hash>` with a hub-signed, short-lived token that encodes member id and licence check) or, if a server has no public HTTPS, via a signed-URL relay through another server. Which mechanism is primary is decided in ADR-004; this ADR requires that both preserve licence enforcement (`owner_only` artifacts are viewable in-Hive, not redistributable, ADR-011).
12. **Storage capability is registered (D50).** Regional servers register `storage_gb_offered`, `bandwidth_mbps`, and `region`; the coordinator places replicas within offered capacity and earns them $honey for bytes × time at the reduced storage rate (ADR-002).

Schema sketch (schema `hive`):

```sql
create table hive.artifacts (
  hash bytea primary key,                              -- blake3 of chunk manifest
  size_bytes bigint not null, mime text not null,
  kind text not null check (kind in ('output','checkpoint','model','input')),
  project_id uuid references hive.projects(id),        -- null for kind='model'
  card_id uuid references hive.cards(id),
  lease_id uuid references hive.leases(id),
  license_kind text check (license_kind in ('owner_only','open_source')),
  replication_factor smallint not null default 2,
  state text not null default 'pinned'
    check (state in ('pinned','grace','returned','evicted')),
  grace_until timestamptz, created_at timestamptz not null default now()
);
create table hive.artifact_replicas (
  hash bytea references hive.artifacts(hash) on delete cascade,
  server_id uuid references hive.regional_servers(id),
  verified_at timestamptz, healthy boolean not null default true,
  primary key (hash, server_id)
);
alter table hive.regional_servers
  add column storage_gb_offered int not null default 0,
  add column bandwidth_mbps int, add column uptime_score numeric(5,4);
```

Artifact lifecycle:

```
upload ──► pinned ──(wallet cannot cover next period)──► grace ──► returned ──► (resubmit, same hash) ──► pinned
                 ▲                                          │
                 └──────────(funding restored)──────────────┘        evicted = replicas deleted, row kept for dedup
```

## Consequences

### Positive
- Zero artifact egress on Supabase; the 250 GB cap is irrelevant to media traffic.
- Bytes sit in-region with members and nodes; same-region fetch (D58) makes browsing renders and pulling model weights fast.
- Content addressing gives free dedup, integrity verification on every read, and a stable id that survives eviction and resubmission.
- Storage becomes a second, cheaper way to earn $honey, which encourages the day-1 target of at least 5 regional servers (Q16) and beyond.
- Checkpoints and model weights ride the same pipeline: one storage implementation to harden.

### Negative
- Durability depends on volunteer hardware; replication factor 2 with volunteer uptime is weaker than an object store's 11 nines.
- Funded pinning means unpopular but valuable work can be evicted; an offline owner can lose an artifact after the grace period.
- The owner-return path needs the owner's node online at some point during grace, or a web download of possibly tens of GB.
- Operating a scrub loop, repair loop and meter adds coordinator complexity beyond scheduling.

### Risks & mitigations
- **Both replicas offline simultaneously.** Mitigation: replicas placed in different regions; repair loop triggers on the first drop; consider `replication_factor = 3` for `output` artifacts of `open_source` projects once server count allows.
- **Storage spam / abuse of dedup.** Mitigation: uploads are tied to a lease or an owner action and debited to a funded project; `kind='model'` pins are hub-only.
- **Corrupt or malicious servers.** Mitigation: per-chunk hash verification, scrub sampling, earnings forfeiture, and automatic replica removal from servers with repeated failures.
- **Licence leakage through the gateway.** Mitigation: every gateway token is minted by the hub after checking membership and licence; tokens are short-lived and bound to a single hash.
- **Grace-period surprise.** Mitigation: web banner and node inbox at grace start, reminder at 50% and 90% of grace, and a projected-funding warning before the wallet runs dry.

## Open questions
- What is the storage rate relative to the compute rate? Open (D50 principle: storage < compute; a server that also donates compute earns more). Owned by ADR-002.
- What is the default grace period? Default assumption: 14 days, configurable per artifact kind.
- Should the gateway be a regional server exposing HTTPS directly, or a signed-URL relay? Open — ADR-004; both must enforce licence.
- Is `replication_factor = 2` adequate for `kind='model'` weights, or should popular models be replicated per region on demand? Default assumption: per-region on demand, minimum 2.
- Does bytes-served (bandwidth) earn $honey in addition to bytes-stored? Open — Q14 says "possibly"; default assumption: stored only in v1.
- Minimum uptime score to hold a sole replica? Default assumption: 0.95 over the trailing 30 days.

## Related
- ADR-001-hub-and-source-of-record
- ADR-002-honey-economics
- ADR-003-node-core-and-backends
- ADR-004-p2p-overlay-and-regional-servers
- ADR-005-scheduler-and-leases
- ADR-006-agent-runtime-and-sandbox
- ADR-009-web-app
- ADR-010-node-desktop-app
- ADR-011-ownership-and-licensing
