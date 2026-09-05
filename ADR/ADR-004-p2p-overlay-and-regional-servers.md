# ADR-004: P2P Overlay and Regional Servers

**Status:** Proposed · **Date:** 2026-09-04 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** ADR-000 Q6, Q8, Q14, Q16, Q17 — D21, D28, D49, D56–D59; implications of Q14 and Q17

## Context

OH Hive's authoritative state lives in Supabase (ADR-001), but the bulk of what moves through the network is not state: it is job payloads, checkpoints, model weights of 5–30 GB, and rendered artifacts (video, audio, images). Routing that traffic through Supabase would exhaust the Pro tier's 250 GB/month egress in hours at the target scale of 2,000 nodes, and Postgres is the wrong tool for it anyway. The interview settled on a peer-to-peer overlay for data movement, with member-volunteered **regional servers** as the preferred relays, bootstrap points, artifact store, and model cache.

Jack's stated requirement is that the network be self-healing: regional servers are preferred infrastructure, but any node with sufficient connectivity can be promoted to a relay when a regional server drops, and the node app already contains the server core (ADR-003, one core two shells). The infra role is explicitly named "regional server" (D21), which implies geographic awareness — members are on every continent, and both artifact fetches and relay selection should prefer the nearest server. At least five regional servers are expected on day 1.

Scale also shapes the control plane. Two thousand nodes each holding a Supabase Realtime connection and writing heartbeats directly to Postgres is neither affordable nor safe (a raw Supabase JWT with write scope on every volunteer machine). Control traffic therefore flows to an elected coordinator over the overlay, the coordinator batches writes into Postgres, and nodes hold only short-lived tokens the coordinator mints.

Two implications surfaced in Q14 and Q17 belong here rather than in the storage or backend ADRs: web-app members browsing projects need a path from HTTPS into the overlay to stream artifacts, and regional servers must double as a model-weight CDN so that 2,000 nodes do not all pull from Hugging Face on launch day. The first is left open; the second is decided.

## Decision

1. **libp2p overlay in `ohhive-core` (D28).** Transport: QUIC primary, TCP fallback, both with Noise encryption. Peer discovery via Kademlia DHT seeded by regional servers; NAT traversal via AutoNAT + DCUtR hole-punching; Circuit Relay v2 for peers that cannot be reached directly. Every node and regional server has a stable libp2p `PeerId` bound to its `hive.nodes` / `hive.regional_servers` row at registration.
2. **Regional servers are preferred relays and bootstrap points (D21, D28).** `hive-server` always runs as a Relay v2 hop and DHT server node. Compute nodes run as DHT clients by default.
3. **Self-healing relay promotion (D28).** The coordinator monitors reachability of regional servers per region. When a region falls below a minimum relay count, the coordinator promotes eligible nodes (public reachability confirmed by AutoNAT, `bandwidth_mbps` above threshold, uptime history above threshold, member opt-in flag `may_relay`) to relay mode for that region, and demotes them when a regional server returns. Promotion is a runtime message, not a reinstall.
4. **Region awareness (D58).** Nodes and regional servers register a `region` (derived from IP geolocation at registration, editable by the member). The overlay prefers same-region relays and same-region artifact/model sources; compute placement is capability-first, region-second (ADR-005).
5. **Bootstrap seed list (D28, Q16 implication).** On startup a node fetches `hive.regional_servers WHERE status='online'` (multiaddrs + region) via an anon-readable RPC, caches it locally, and falls back to a hard-coded list of the five day-1 regional servers compiled into the binary. Order of attempts: cached same-region → cached any → compiled-in seeds. Seeds are rotated by release; the compiled list is a last resort, not the primary path.
6. **Short-lived hub tokens (Q16 implication).** Nodes never hold a Supabase JWT with write scope. At check-in a node authenticates to the coordinator (member JWT proves identity once; the `PeerId` is bound to it), and the coordinator mints a hub token (signed, ≤ 15 min TTL, scoped to `node_id` and current lease ids) recorded in `hive.hub_tokens`. All node → hub writes go through the coordinator, which is the only overlay participant holding a Postgres credential (ADR-001 decision 10).
7. **Heartbeat batching via coordinator (D59).** Nodes heartbeat to the coordinator over the overlay every 10 s (per-modality overrides for long jobs, ADR-005). The coordinator aggregates and writes `hive.nodes.last_seen` / `hive.leases.renewed_at` in batched `UPDATE ... FROM unnest(...)` statements at ≤ 1 Hz, bounding Postgres write load to a handful of statements per second regardless of fleet size. Supabase Realtime is not used for any node control traffic.
8. **Coordinator election (D57).** One regional server holds `hive.coordinator_lease` (ADR-001). Non-coordinator regional servers run in standby: they maintain relay/DHT/storage duties and watch the lease; on lapse they contend for it. Nodes discover the current coordinator through a DHT record signed by the lease epoch, so failover requires no node restart.
9. **Regional servers as artifact store (D49).** Artifacts are content-addressed (BLAKE3 hash) and stored on regional servers' offered disk (`storage_gb_offered`). `hive.artifacts` holds metadata, hash, size, and replica locations only. Supabase Storage is not the artifact store. Storage policy detail (funded pinning, grace, owner return) is ADR-007.
10. **Replication factor 2 (Q14 implication).** Default: two replicas on distinct regional servers, preferring one in the owner's region and one elsewhere. When a server drops below liveness thresholds, the coordinator schedules re-replication from the surviving replica. Servers below a minimum uptime score never hold a sole replica.
11. **Regional servers as model cache / CDN (Q17 implication).** Model weights (`ModelRef`, ADR-003) are distributed through the same content-addressed store: regional servers fetch from upstream (Hugging Face or a Hive mirror) once, and nodes pull from the nearest regional server over the overlay, optionally seeding to same-region peers. Popular models may be pre-positioned on all regional servers before launch.
12. **Control-channel protocol.** Node ↔ coordinator RPC is gRPC over the libp2p QUIC stream (protocol id `/ohhive/ctl/1`), carrying check-in/out, capability updates, lease claim/renew/release, checkpoint pointers, and hub-token refresh. Data transfer (artifacts, models, checkpoints) uses a separate block-exchange protocol (`/ohhive/blocks/1`) so bulk transfer never head-of-line-blocks control messages.
13. **Integrity on fetch.** Every block fetched from a peer or server is verified against its hash before use; servers that serve corrupt data are marked and lose storage earnings for that period (ADR-002).

## Consequences

### Positive
- Bulk data never touches Supabase, keeping DB size and egress within the shared Pro tier.
- Volunteers' Pi-class servers can act as relay, bootstrap, and storage without any inference dependency.
- Self-healing promotion makes the network tolerant of individual server operators checking out, which is expected behaviour, not an incident.
- Short-lived hub tokens mean a compromised node yields a 15-minute, node-scoped credential rather than database write access.
- Batched heartbeats decouple Postgres load from fleet size.

### Negative
- libp2p brings a large dependency surface and its own operational learning curve (NAT edge cases, relay capacity limits, DHT churn).
- Relay bandwidth is donated by members; heavy video artifacts through relays can saturate a home connection.
- The coordinator is a single logical point of dispatch; failover is automatic but pauses new dispatch for the lease TTL.
- Region derived from IP is imprecise (VPNs, mobile hotspots) and requires a manual override path in the UI.

### Risks & mitigations
- **Relay overload.** Mitigation: Relay v2 reservation limits per peer and per relay; prefer direct connections via hole-punching; artifacts are streamed from the nearest server that holds a replica rather than relayed.
- **DHT poisoning / rogue peers.** Mitigation: invite-only membership plus `PeerId` ↔ member binding at registration; coordinator publishes signed records; nodes reject unsigned or wrong-epoch coordinator records.
- **Bootstrap failure on a fresh install.** Mitigation: three-tier seed order (cached-region, cached-any, compiled seeds); compiled seeds refreshed each release; seed health monitored.
- **Data loss when both replicas' servers leave.** Mitigation: uptime-scored placement, re-replication on first liveness failure, and never placing both replicas on servers with low scores; replication factor is a config value that can be raised.
- **Launch-day model pull storm.** Mitigation: pre-position the default text model and the top image/video checkpoints on every regional server before invites go out; nodes rate-limit initial pulls.

## Open questions
- **Web-app gateway.** How do browser sessions stream artifacts from the overlay? Options: (a) regional servers expose an HTTPS gateway with signed URLs minted by the hub; (b) a signed-URL relay via the coordinator; (c) a WebTransport/WebRTC libp2p path directly from the browser. Open; default assumption for planning is (a), with TLS certificates provisioned automatically for servers that have a public hostname.
- Do regional servers hold replicas for members outside their region by default, or only on overflow? Default: one replica same-region, one anywhere.
- Minimum bandwidth and uptime thresholds for relay promotion and for sole-replica eligibility? Default: 50 Mbps up, 95% 30-day uptime; to be tuned from day-1 telemetry.
- Should nodes seed model weights to peers (BitTorrent-style) or only fetch from servers? Default: fetch from servers in v1, peer seeding behind a flag.
- Coordinator hub-token TTL: 15 min default; is refresh-on-heartbeat sufficient or does it need a separate refresh RPC? Default: piggyback on heartbeat.
- Is BLAKE3 the hash, or SHA-256 for tooling familiarity? Default: BLAKE3 for speed on Pi-class servers.
- How is region geolocation sourced without a paid service? Default: MaxMind GeoLite2 bundled in `hive-server`, refreshed monthly.
- How many regional servers per continent are needed before launch beyond the five committed? Open — depends on where the day-1 five are.

## Related
- ADR-001-hub-and-source-of-record.md — coordinator lease, `hive.regional_servers`, `hive.hub_tokens`, Realtime split.
- ADR-002-honey-economics.md — storage and egress earnings for server operators; integrity penalties.
- ADR-003-node-core-and-backends.md — `ohhive-core` hosts the libp2p stack; `hive-server` footprint; model refs.
- ADR-005-scheduler-and-leases.md — heartbeat cadence, lease TTLs, region-second placement.
- ADR-006-agent-runtime-and-sandbox.md — checkpoint pointers travel over the control channel.
- ADR-007-artifact-storage.md — funded pinning, grace, owner return, dedup on the store described here.
- ADR-008-auth-and-membership.md — member JWT exchanged for hub token at check-in; `PeerId` binding at registration.
- ADR-009-web-app.md — consumer of the gateway decision.
- ADR-010-node-desktop-app.md — `may_relay` toggle, region override, pending-returns inbox.
- ADR-011-ownership-and-licensing.md — servers hold `owner_only` material; ToS for operators.
- ADR-012-scope-and-roadmap.md — peer model seeding and browser-native libp2p as later items.

## Appendix A — Protocol identifiers and roles (non-normative)

| Protocol id | Purpose | Participants |
|---|---|---|
| `/ohhive/ctl/1` | gRPC control channel: check-in/out, capabilities, leases, checkpoint pointers, token refresh | node ↔ coordinator |
| `/ohhive/blocks/1` | content-addressed block exchange for artifacts, checkpoints, model weights | node ↔ server, server ↔ server, node ↔ node (flagged) |
| `/ohhive/coord/1` | signed coordinator announcement record in the DHT (`epoch`, `PeerId`, multiaddrs) | coordinator publishes, all read |
| libp2p `kad` | peer discovery | servers as DHT servers, nodes as clients |
| libp2p `relay/2` | circuit relay for unreachable peers | servers always; promoted nodes on demand |
| libp2p `autonat` + `dcutr` | reachability probe and hole-punching | all |

| Role | Binary | Relay | DHT | Storage | Coordinator eligible | Inference |
|---|---|---|---|---|---|---|
| Regional server | `hive-server` | always | server | yes | yes | no |
| Compute node | `hive` (in Tauri) | on promotion | client | no (v1) | no | yes |
| Promoted node | `hive` | yes | server | no | no | yes |
