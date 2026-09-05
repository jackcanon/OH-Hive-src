# OH Cloud — ADR Interview Log

**Project:** OH Cloud ("the Hive") — distributed local-LLM compute network
**Owner:** Jack Blair, Happy Jack Media
**Scribe:** Loki
**Started:** 2026-09-04
**Status:** In progress — living document, updated after every answer

## Purpose

Running log of the architecture interview. Every question and answer is captured here as it happens so no work is lost. Individual ADRs (ADR-001+) will be split out from this log once decisions solidify.

## One-line vision (Jack, opening statement)

An app that lets users across the world who are part of the "Hive" connect their computers to run local large language models together.

---

## Interview

### Q1 — Who is the Hive, and what does a member get out of contributing their machine?

**Answer (Jack):**

- **Communities served:** Office Hours Global and Loki's Lab.
- **Membership:** Invite-only, at least initially.
- **Two ways to join:**
  1. Register a computer as a **compute node** (contributes LLM inference).
  2. Volunteer **server hardware** to keep the peer-to-peer network / cloud infrastructure running.
- **Credit system:** A token/credit based on LLM tokens generated. Working name: **$honey** (bee/hive branding TBD — depends how far the metaphor is committed to).
- **Earning $honey — only two ways:**
  1. Register and contribute a machine (compute or infra).
  2. Pay for API-based cloud agents (purchase path).
- **Spending $honey:** On compute for **projects**. Projects live on a **kanban board on a website**. Contributors can create project boards. A project's work is processed in proportion to the tokens allocated to it — more $honey = faster processing.
- **Node availability:** Contributors **check in / check out** a machine at will, or set a **scheduled run time**.
- **Two software distributions:**
  1. **Lightweight server app** — infrastructure-only participant in the Hive.
  2. **Full app** — server components + client compute node.

**Decisions captured:**
- D1. Invite-only at launch.
- D2. Token economy ($honey, provisional) backed by LLM tokens generated; earned via compute contribution or API-agent purchases; spent on kanban projects.
- D3. Two distributions: infra-only server, and server+compute node.
- D4. Node presence is opt-in with manual check-in/out and scheduling.

**Open questions raised by this answer (to revisit):**
- Exchange rate: how many $honey per generated token, and does it vary by model size / hardware class?
- What exactly is an "API-based cloud agent" purchase — buying credits with fiat, or buying hosted agents that then earn?
- Does the $honey ledger need to be tamper-resistant across peers (trust model)?

### Q2 — What does a "project" on the kanban actually run?

**Answer (Jack):**

- **Scope of work:** Anything an AI agent can do — video, images, code, music, any creative/technical output. "Anything we can create together, we create together as the Hive."
- **Roles per project:**
  - **Project Owner** — dictates the project; builds it via a general chat with an agent; can invite Admins.
  - **Admins** — decide what goes on project cards; have **dedicated compute** allocated to the project.
  - **Followers** — can *suggest* cards (Admins accept/reject).
- **Visibility:** All work is open to every Hive member. Any member can inspect any project. "Almost open source" — but nothing is exposed to the open internet / non-members.

**Decisions captured:**
- D5. Project = agentic, multi-modal workload (not a single prompt). Cards are agent tasks.
- D6. Three-tier project RBAC: Owner → Admin → Follower.
- D7. Owner authors the project conversationally with an agent (chat-to-project).
- D8. Hive-internal transparency: read-all for members, zero external exposure.

**Implications flagged:**
- Multi-modal workloads mean nodes must advertise **capabilities** (text LLM, image, video, audio, GPU VRAM, etc.), and the scheduler must match cards to capable nodes.
- "Dedicated compute for Admins" implies compute **reservation/priority**, not just a $honey queue.
- Read-all-inside / nothing-outside implies auth at the network edge (member identity gates all reads) and encrypted transport between peers.

### Q3 — Where does the truth live?

*(Options offered: central coordinator; fully P2P ledger/CRDT; hybrid.)*

**Answer (Jack):** Use the **Cmd Work Supabase** project.

**Decisions captured:**
- D9. **Hybrid, hub-centric.** Source of record = Supabase (Postgres) — the existing Cmd Work instance. Identity (Supabase Auth), $honey ledger, projects/kanban, job queue, node registry all live there. Peers do compute and data transfer; they do not hold authoritative state.
- D10. Reuse existing infra rather than stand up a new Supabase project. OH Cloud tables get their own schema/prefix inside Cmd Work's DB to avoid collisions.

**Implications flagged:**
- Supabase Realtime can drive node presence (check-in/out heartbeats) and kanban live updates.
- Row Level Security enforces D8 (member-only reads) at the DB edge.
- The $honey ledger should be append-only (double-entry style) with balances derived, not stored — cheap tamper-resistance without a blockchain.
- Job dispatch: nodes poll or subscribe to a `jobs` table; hub assigns, node claims with a lease, results written back. Edge Functions can host the scheduler.
- Large artifacts (video, models) should NOT go through Postgres — Supabase Storage or direct peer transfer. To decide.

### Q4 — What is a compute node, technically?

**Answer (Jack):**

- Runtime: "whatever is the most efficient way to run an LLM." No runtime mandated — pick on merit.
- **Goal: a user-distributed model.** Every registered computer is contributing to *the* LLM.
- **Client OS support at launch:** macOS, Linux, Windows 10+.
- **Server (infra) app footprint:** must run on anything from a **Raspberry Pi** to an **HP rack server**.

**Decisions captured:**
- D11. Cross-platform from day one: macOS / Linux / Windows 10+ for the compute node.
- D12. Infra-only server must be tiny and portable (ARM64 + x86_64, low RAM). Strong hint toward a single static binary (Go or Rust), not Electron/Node.
- D13. Inference runtime is a pluggable backend chosen for efficiency per platform, not a fixed dependency. Candidates: llama.cpp (broadest hardware, GGUF), MLX (Apple Silicon), vLLM/exllama (NVIDIA), with Ollama as a convenience wrapper. Decide after Q5.

**Implications flagged:**
- Pi-class servers rule out heavy runtimes for the infra role; the infra role is coordination/relay/storage, not inference.
- "Whatever is most efficient" per platform + cross-platform = a thin **node agent** (Go/Rust) that shells out to / links a backend, rather than one monolithic runtime.
- GUI question (desktop app vs headless daemon + web dashboard) still open — deferred.

**Critical fork surfaced:** "user-distributed model" has two very different meanings — see Q5.

### Q5 — Distributed *inference* or distributed *capacity*?

*(A = each node runs whole models it can fit, jobs routed to capable nodes. B = one large model sharded across peers, Petals/exo style.)*

**Answer (Jack):** "AB" — interpreted as **A first, B on the roadmap.**

**Decisions captured:**
- D14. **v1 = distributed capacity (A).** Nodes run complete models locally; the hub routes cards to nodes whose advertised capabilities fit. Ships fast, tolerant of nodes checking out.
- D15. **v2 = distributed inference (B)** as a first-class roadmap item. Architecture must not preclude it: node agent needs a pluggable "execution mode," and the capability schema should already record RAM/VRAM/bandwidth so shard planning is possible later.
- D16. Backend choice for v1 (resolves D13): **llama.cpp** as the universal engine (GGUF, CPU+Metal+CUDA+Vulkan, all three OSes), with **MLX** as an optional accelerator on Apple Silicon. exo is the leading candidate to evaluate for v2 since it already does heterogeneous-device sharding and can sit on top of the same node agent.

**Implications flagged:**
- Job schema needs `required_capabilities` (model, min VRAM, modality) so A works, and `shard_plan` (nullable) so B can slot in without a migration.
- Model distribution: nodes must pull GGUF weights; a shared model catalog in Supabase + Storage/CDN or peer seeding.

### Q6 — What is the "cloud" in OH Cloud? (The API-agent purchase path)

**Answer (Jack):**

- Members can buy API credit from **any provider** — Claude, Codex/OpenAI, Nous, etc.
- That credit becomes $honey that can be **donated to community projects** or used to **fund their own**. Example: buy $100 of API credit → $25 to support someone's project, $75 to start your own.
- Buying API tokens is itself a valid path to **membership**.
- **Three ways to join/support the Hive** (canonical list):
  1. Buy 3rd-party API tokens.
  2. Register your computer as compute.
  3. Provide **regional server** hardware for the P2P network.

**Decisions captured:**
- D17. **Two compute pools, one currency.** Cards can execute on (a) Hive local nodes or (b) 3rd-party provider APIs. $honey is the single unit across both; the scheduler picks the pool per card.
- D18. **Provider-agnostic API layer.** Claude, OpenAI/Codex, Nous, and future providers sit behind one adapter interface. Provider API keys are held by the hub (Supabase Vault / Edge Function secrets), never on member nodes.
- D19. **$honey is transferable within the Hive** — a member may allocate their balance to any project (theirs or others'). Ledger entries: `purchase`, `earn_compute`, `earn_infra`, `fund_project`, `spend_job`.
- D20. Three membership on-ramps: purchase, compute, regional infra. Any one qualifies (subject to invite).
- D21. The infra role is named **"regional server"** — implies geo-aware P2P relay/storage, not just a generic node.

**Implications flagged:**
- Purchases need a payment rail (Stripe) → fiat in, hub buys provider credit, $honey issued. Is the hub the reseller (member pays Hive, Hive pays provider) or does the member bring their own key? Default assumption: **hub is reseller**, keys centralized, per D18. Confirm.
- Exchange rate must now reconcile two things: $honey per local generated token (D2) vs $honey per fiat dollar. Provider token prices differ wildly, so $honey should be pegged to a **cost unit**, and each pool/model gets a price in $honey.
- Regional servers = natural relay for NAT traversal, model-weight caching, and artifact storage near members.

### Q7 — The $honey exchange rate

**Answer (Jack):**

- **Priority is local compute.** The network's value is *volume* — "many of us have extra compute hours and assets that sit idle off-peak."
- **You earn what you generate.** No uptime bonus, no hardware-class multiplier. Generate tokens → earn their value. Earn as much as you can; spend it to get your own work done fast.
- **Peg: Anthropic's token price.** Whatever a token of compute costs at Anthropic is what contributors earn per token generated.

**Decisions captured:**
- D22. **$honey is pegged to Anthropic's per-token price.** 1 token generated on a Hive node earns the $honey equivalent of 1 Anthropic output token. (Which Anthropic model sets the reference rate is an open detail — default: Sonnet-tier, revisit.)
- D23. **Pure output-based reward.** No multipliers. Earnings = tokens generated × reference rate.
- D24. **Local-first scheduling.** The scheduler prefers Hive nodes; provider APIs are the overflow/fallback pool, not the default.
- D25. The economic model is a **time-shifting compute bank**: contribute idle cycles, withdraw burst capacity later.

**Implications flagged:**
- Pegging to an external price means the rate is a **config value with history** (rate table with effective dates), not a constant. Ledger entries record tokens *and* the rate applied at the time.
- "You earn what you generate" requires **verifiable token counts**. The node agent must report counts the hub trusts — at minimum: hub-assigned jobs only (no self-reported freelance work), token counts derived from the job output the hub receives, and spot-check/replay for fraud detection. Invite-only membership lowers the risk but the design should not depend on it.
- Input vs output tokens: does prompt processing earn? Default assumption: **output tokens earn full rate, input tokens earn a fraction** (mirrors Anthropic's own input/output pricing). Confirm.
- Multi-modal cards (image/video/audio) have no "tokens." Need a conversion: price those by **compute-seconds normalized to the token rate**, or a per-artifact tariff. Decide in a dedicated ADR.

### Q8 — What does a member actually see and touch?

**Answer (Jack):** Confirmed the three surfaces. Additions:

- The **node app contains the server** — any member's node can become a server for the other nodes if needed. **The network is self-healing.**
- The node app is a **proper desktop application**: dock/taskbar icon, launcher, full GUI. Shows check-in/out, schedule, hardware stats, earnings.

**Decisions captured:**
- D26. **Three surfaces:** (1) Web app — kanban, project chat, wallet, Hive browser. (2) Node desktop app — GUI, tray/dock presence. (3) Regional server — headless binary.
- D27. **One core, two shells.** The regional server and the node app share the same core binary (the "hive core": P2P networking, relay, job runner). The regional server *is* that core run headless; the node app wraps it in a GUI and adds the inference backend. This is how "any node can become a server" costs nothing extra.
- D28. **Self-healing P2P overlay.** Regional servers are preferred relays/bootstrap points, but any node with sufficient connectivity can be promoted to relay when a regional server drops. Supabase remains the source of record (D9); the overlay is for job data and artifact transfer, not authority.
- D29. Node app is a **native-feeling desktop app** on all three OSes. Leading candidate: **Tauri** (Rust core matches D12/D27; small binary; web UI can share components with the web app). Electron is the fallback if Tauri's platform quirks bite.

**Implications flagged:**
- Language for the core is now effectively decided by D12 + D27 + D29: **Rust** (static binaries for Pi→server, Tauri shell, libp2p available). Go is the alternative if Rust velocity is a concern — decide in ADR-00x.
- Self-healing implies a **peer discovery + role election** protocol. libp2p (Kademlia DHT, relay v2, NAT hole-punching) covers most of this off the shelf.
- Shared UI: web app and Tauri app can share a component library (React/Svelte). Pick one framework for both.

### Q9 — Which parts of the web stack are already decided by Cmd Work?

**Answer (Jack):** Separate app that only shares the **database and auth**. Loki to inspect `CmdWork-src` for the rest.

**Findings from `Apps/CmdWork-src` (Loki, 2026-09-04):**
- Cmd Work is a **native Swift app** (macOS + iOS, XcodeGen `project.yml`), `supabase-swift` client. There is **no web app yet** — `CmdWork-WebApp/` holds only an Apple Developer key. ARCHITECTURE.md plans "Next.js or SvelteKit on Vercel" as a follow-on; `cmdwork.app` domain is secured.
- **Auth:** Supabase Auth with Sign in with Apple + Google, federated to one user; `public.profiles.id == auth.users.id`. Web SIWA will need a separate Apple **Services ID** + redirect URL (one-time portal setup).
- **Schema (`supabase/schema.sql`, all in `public`):** `profiles`, `projects` (goal/status/health/memory/deleted_at), `memberships` (roles `owner|admin|member|viewer`, `pending|active`, email invites), `work_items` (status `todo|doing|ready_for_review|done`, priority, labels[], assignee, agent_id, deleted_at), `agents` (provider `claude|chatgpt|gemini|custom`), `decisions`, `work_item_comments` + `comment_reactions` + mentions, `notifications` via triggers.
- **RLS pattern:** `is_project_member(pid)` / `is_project_admin(pid)` SECURITY DEFINER helpers; members read/write their projects only. Trigger auto-adds creator as owner. Realtime enabled on main tables.
- **Sibling:** `Apps/cmdwork-agent-mcp-src` — an MCP server that lets agents read/write Cmd Work (the one Loki has in this session). Rule: agents may not set `done`, only `ready_for_review`.
- **Economics note:** Supabase Pro $25/mo, 8 GB DB, 250 GB egress — egress cap matters if artifacts flow through Supabase Storage.

**Decisions captured:**
- D30. OH Cloud web app is a **separate codebase and deployment**, sharing only the Supabase project (DB + Auth). It does not live inside Cmd Work's UI.
- D31. Auth is inherited as-is: Supabase Auth, Apple + Google, `profiles` row is the member identity. OH Cloud adds a `hive_members` table (invite status, membership on-ramp, wallet id) keyed to `profiles.id`.
- D32. OH Cloud tables go in their **own Postgres schema `hive`** (not `public`) to keep Cmd Work's grants/RLS/realtime untouched and make the boundary auditable.
- D33. Web framework: **Next.js on Vercel** (matches Cmd Work's stated plan, Vercel connector available, `supabase-js`). UI library shared with the Tauri node app via a common React component package.

**Open question this raises → Q10.**

### Q10 — Is the Hive kanban Cmd Work's kanban, or its own?

**Answer (Jack):** Own tables in `hive`, with an optional one-way "mirror to Cmd Work" later.

**Decisions captured:**
- D34. Hive projects/cards are **independent tables in schema `hive`** (`hive.projects`, `hive.cards`, `hive.project_roles`). Not Cmd Work rows.
- D35. Hive RLS: any active `hive_members` row can **read** every project/card (D8); writes gated by `hive.project_roles` (owner/admin) and a `suggest` path for followers.
- D36. **Mirror to Cmd Work** is a later, one-way, opt-in sync (Hive → `public.projects`/`work_items`), so Cmd Work and its agent MCP can view Hive work. Not v1.

### Q11 — What does a card's execution look like end to end?

**Answer (Jack):** Member opens the web app, types a prompt saying they want to start a project. An **interviewer agent** interviews them (**this costs $honey**). The output of the interview is the **project plan, materialized as its own project with a kanban** that all nodes can work.

**Decisions captured:**
- D37. **Project creation = agent interview.** Prompt → interviewer agent (chat) → structured project plan → `hive.projects` + `hive.cards` seeded from the plan. The interviewer is the same "chat-to-project" surface from D7.
- D38. **Interviews are metered.** The interview itself is a job that burns $honey from the member's wallet (charged per token like any other work). First $honey-spend touchpoint in the product.
- D39. **Planning lives on the hub.** The interviewer/planner agent runs hub-side (Edge Function or a small hub worker) because it needs to write projects/cards with authority. It may *execute* on a Hive node or a provider API per D24 (local-first), but it is hub-orchestrated.
- D40. Cards produced by the plan are **typed** (modality, required capabilities, dependencies) so the scheduler can dispatch them without a human breaking them down.

**Implications flagged:**
- The interview needs a **structured output contract** (JSON schema: project title, goal, cards[] with type/inputs/deps/acceptance). That schema is the first shared type between web app, hub, and node core.
- Card dependencies form a DAG → the kanban is a view over a DAG, and "In Progress" means "a node holds a lease on it."
- Still unresolved: once a node picks up a card, does it run the whole card as a local agent loop, or does the hub pre-split cards into atomic jobs? → Q12.

### Q12 — Card = one node's agent loop, or hub-split atomic jobs?

**Answer (Jack):** Yes — node-owned agent loop with checkpointing.

**Decisions captured:**
- D41. **One card, one node, one agent loop.** A node claims a card with a lease and runs the full agent loop locally (plan → generate → tool calls → self-review → done).
- D42. **Checkpointing is mandatory.** The node writes progress checkpoints (conversation state, intermediate artifacts, step index) back to the hub at step boundaries. If the node checks out or the lease expires, another capable node resumes from the last checkpoint.
- D43. **The agent runtime lives in the Rust node core**, not on the hub. The hub only plans (D39), schedules, and stores. This keeps the hub cheap and puts the tokens where they're generated (and earned, D23).
- D44. **Sub-delegation allowed.** A card's agent loop may spawn child jobs for capabilities the node lacks (e.g. a text-only node needs an image) — child jobs go back through the hub scheduler to a capable node. Parallelism comes from the card DAG (D40) plus child jobs, not from splitting prompts.

**Implications flagged:**
- Accounting: tokens are metered per node per card-lease; a resumed card accrues earnings to each node for the segment it ran.
- Lease + heartbeat semantics: `hive.leases(card_id, node_id, expires_at)`; node heartbeats every N seconds; scheduler reaps expired leases.
- Checkpoint payloads can be large → store in Supabase Storage / regional server, keep only pointers in Postgres.
- Tool access inside the loop (web fetch, file I/O, code exec) needs a **sandbox policy** on the node — contributors are running other members' agent tasks on their own machine. Dedicated ADR.

### Q13 — Trust and safety on a contributor's machine

**Answer (Jack):** Yes — inference + sandboxed tools by default, with a per-node restriction toggle. Plus:

- **Internet access is a separate, explicit opt-in.** Node contributors choose at **node setup/registration** whether their node may give projects internet access. **Off by default.**
- The opt-in is **per node, whole-node** (the contributor opts the entire compute node in or out).
- **Projects declare** whether they require internet or not.

**Decisions captured:**
- D45. **Default execution policy:** local inference + sandboxed tools (scratch dir, code exec in sandbox). No network from inside the sandbox unless D46 applies.
- D46. **Internet is a node-level capability flag**, `allow_internet: bool`, default `false`, set during node registration and changeable in the node app. Not per-project, not per-card — whole node.
- D47. **Projects/cards declare `requires_internet: bool`.** The scheduler only matches internet-requiring cards to nodes with `allow_internet = true`. A card never gets network it didn't declare, even on an opted-in node.
- D48. Contributors also get a **`tools` level** toggle: `inference_only` | `sandboxed_tools` (default). Same registration-time flow, same scheduler matching.

**Implications flagged:**
- Node capability record now includes: hardware (RAM/VRAM/GPU), models available, modalities, `allow_internet`, `tools_level`, schedule. This is the scheduler's matching input.
- Sandbox tech: on macOS/Linux/Windows the portable answer is **WASM (wasmtime)** for agent tools + a network shim that is a no-op unless `allow_internet`. Containers are a fallback on Linux/servers only.
- The interviewer agent (D37) must ask "does this project need the internet?" and set the flag — otherwise cards silently starve for nodes.
- UI: node app must make the two toggles obvious at setup and in Preferences; web app shows a card's badge (🌐 required) and the count of eligible nodes.

### Q14 — Artifacts and storage

**Answer (Jack):** Option **B — regional servers** are the artifact store. Server registrars earn $honey at a **reduced rate** (scaling TBD; a server that also donates compute will earn more than a storage-only server). **Unfunded artifacts are returned to the project owner** for self-hosting; once funding is available again the owner can resubmit the artifact.

**Decisions captured:**
- D49. **Artifacts live on regional servers**, addressed by content hash. Postgres (`hive.artifacts`) holds only metadata + hash + replica locations. Supabase Storage is *not* the artifact store (egress cap, cost).
- D50. **Storage earns $honey at a reduced rate.** Regional server operators earn for bytes stored × time (and possibly bytes served). Exact rate vs compute rate is an open economics ADR; principle fixed: storage < compute.
- D51. **Pinning is funded.** An artifact stays on the Hive only while its project has $honey to cover storage. When funding runs out, the artifact enters a **grace period**, then is **returned to the project owner** (downloaded to their node / offered for download) and evicted from regional servers.
- D52. **Resubmission.** Owner can re-upload the artifact once the project is funded again; same content hash → deduplicated.

**Implications flagged:**
- Replication factor: default **2 replicas on distinct regional servers** so a single server checking out doesn't lose data. Scheduler re-replicates when a server drops (self-healing, D28).
- Owner-return path requires the owner's node app to be online at some point during the grace period — the node app needs a "pending returns" inbox and the web app a warning banner.
- Regional server disk contribution is a registered capability: `storage_gb_offered`, `bandwidth_mbps`. Servers below a minimum uptime should not hold sole replicas.
- Integrity: content hashes verify on fetch; servers found serving corrupt data lose storage earnings for that period.
- Members browsing projects (D8) stream artifacts from the nearest regional server via the overlay — the web app needs a gateway path (a regional server exposing HTTPS) or a signed-URL relay. To decide in the P2P ADR.

### Q15 — Ownership and licensing of Hive-made work

**Answer (Jack):** The **project owner owns it.** The interviewer agent establishes at setup whether the project is **open source** or **solely owner-owned**.

**Decisions captured:**
- D53. **Default ownership = project owner.** Compute contributors are paid in $honey (D23) and acquire no rights in the output.
- D54. **License is set at creation by the interviewer** (D37): `license: 'owner_only' | 'open_source'`. For `open_source`, the interviewer also captures the specific license identifier (SPDX, e.g. MIT, Apache-2.0, CC-BY-4.0). Stored on `hive.projects`.
- D55. **Inspect ≠ reuse.** D8's read-all transparency lets members *view* any project; only `open_source` projects may be reused/forked by other members. `owner_only` artifacts are viewable in-Hive but not redistributable.

**Implications flagged:**
- Contributor ToS at node registration must state: you provide compute, you earn $honey, you claim no rights to outputs, and you agree not to redistribute `owner_only` material you can see. Non-technical but blocking for launch.
- License is changeable later by the owner (`owner_only` → `open_source` only; not the reverse once others have forked).
- "Fork project" is a natural web-app feature for `open_source` projects: copies plan + cards + artifact pointers into a new project under the forker.

### Q16 — Scale and launch shape

**Answer (Jack):**
- Members: **could hit 2,000 in one day, or over six months** — Office Hours Global is unpredictable.
- Regional servers: **at least 5 on day 1.**
- **Global community** — members on every continent. First proof project: not yet named.

**Decisions captured:**
- D56. **Design for 2,000 nodes on day 1**, not 50. The scheduler is a **persistent hub worker** (long-running process, Rust, same core crate) — not a request-scoped Edge Function. Edge Functions remain for request/response things (interview turns, Stripe webhooks, invite acceptance).
- D57. **Hub worker runs on a regional server**, not on Vercel/Supabase. One elected **coordinator** among regional servers holds the scheduler lease (Postgres advisory lock / `hive.coordinator_lease` row); any regional server can take over — same self-healing story as D28.
- D58. **Geo-aware from v1.** Nodes and regional servers register a `region` (derived from IP at registration, editable). Scheduler prefers same-region artifact fetch and relay; compute placement is capability-first, region-second.
- D59. Realtime fan-out to 2,000 clients does **not** go through Supabase Realtime for node control traffic (connection limits, cost). Nodes talk to the coordinator over the libp2p overlay / a gRPC-over-QUIC channel; Supabase Realtime is reserved for **web-app UI** updates (kanban, wallet).

**Implications flagged:**
- Postgres load: 2,000 nodes heartbeating every 10 s = 200 writes/s — fine, but batch heartbeats through the coordinator rather than each node hitting Supabase directly. Nodes hold **short-lived hub tokens** minted by the coordinator, not a raw Supabase JWT with write access.
- Bootstrap: the node app needs a list of regional servers to find the overlay — served from Supabase (`hive.regional_servers` where `status='online'`), cached locally, with the 5 day-1 servers as hard-coded fallback seeds.
- Onboarding surge: invite codes + Stripe + node registration must all work with zero manual steps on day 1 or the surge is wasted.
- Need a **first proof project** decided before code — it fixes the v1 modality set (text-only proof = ship weeks earlier than video). Ask Jack.

### Q17 — The first proof project

**Answer (Jack):** All modalities on day 1. The community is **film, audio, television and radio** people — "it could be anything and everything on day 1."

**Decisions captured:**
- D60. **v1 modality set: text, code, image, video, audio (speech + music).** No text-only soft launch.
- D61. **Backend adapter interface is the v1 critical path.** The Rust node core exposes one `Backend` trait (`capabilities()`, `run(job) -> stream`, `usage()`) and ships adapters for:
  - **Text/code:** llama.cpp (all OSes), MLX (Apple Silicon).
  - **Image:** Stable Diffusion / FLUX via **ComfyUI** (managed as a subprocess; API mode) — broadest model coverage, already what this audience uses.
  - **Video:** ComfyUI workflows (Wan / HunyuanVideo / LTX / AnimateDiff class models) — same adapter, different workflow graphs.
  - **Speech:** Whisper (STT) via whisper.cpp; TTS via Kokoro / XTTS / Piper (pick per platform).
  - **Music:** MusicGen / Stable Audio Open / ACE-Step class models — through ComfyUI or a Python sidecar.
- D62. **Python sidecar is permitted on compute nodes** (not on regional servers, D12). Managed by the node app (bundled uv-created venv), not the user's system Python. This is the pragmatic cost of "everything on day 1" — the media ML ecosystem is Python.
- D63. **Nodes advertise per-modality capability**, and the network's day-1 coverage will be uneven (many text nodes, few video nodes). The web app must show per-project "eligible nodes" so owners see why a video card is queued.

**Implications flagged:**
- Non-token modalities need the pricing rule flagged under Q7: **compute-seconds × hardware class, normalized to the Anthropic token rate.** Must be in the economics ADR before launch.
- Model weights for image/video are 5–30 GB each. Regional servers become the **model cache/CDN** (D49 infrastructure reused) so 2,000 nodes don't all pull from Hugging Face at once.
- Video jobs can run 10–60 min; lease/heartbeat timeouts (D42) must be per-modality, and checkpointing for ComfyUI = saving the workflow graph + completed node outputs.
- Windows + NVIDIA is the dominant video-capable platform in this audience; CUDA support on Windows is a first-class test target, not an afterthought.
- Scope risk is real. Mitigation: ship adapters behind the one trait so each modality is independently enable-able, and stage the *launch invites* by modality if the video path lags.

### Q18 — Naming and identity

**Answer (Jack):** Product name is **OH Hive**. Try to buy **ohg-hive.com**.

**Domain check (Vercel, 2026-09-04):**
- `ohg-hive.com` — **available**, $11.25/yr
- `ohghive.com` — available, $11.25/yr
- `ohg-hive.app` — available, $9.99/yr
- `oh-hive.com` — available, $11.25/yr
- `ohhive.com` — taken

**Decisions captured:**
- D64. **Product: OH Hive.** Network/community: "the Hive." Currency: **$honey**. Source folder stays `OH Cloud-src` for now; rename to `OH Hive-src` when the scaffold lands.
- D65. **Primary domain: `ohghive.com`** — **purchased 2026-09-04** via Vercel (team Happy Jack Media, order `01M1R0GATRE48GQB0VM5EYB41K`, $11.25/yr, auto-renew on). Jack chose the no-hyphen form over `ohg-hive.com`. Optional later pickups: `ohg-hive.com`, `ohg-hive.app`.
- D66. **Identifiers:** bundle/app ID `media.happyjack.ohhive`; Rust crate/workspace `ohhive`; Postgres schema `hive`; CLI binary `hive` (node core) / `hive-server` (regional); Tauri app "OH Hive".

### Q19 — What does v1 *not* do?

**Answer (Jack):** Cut list accepted; **mobile app moves to v1.1** (not "someday").

**Decisions captured:**
- D67. **Out of scope for v1:** distributed inference (v2, D15); Cmd Work mirror (D36); $honey → fiat cash-out; public/non-member project pages; training / fine-tuning jobs.
- D68. **v1.1 = mobile app** (iOS + Android). Scope: wallet, kanban view, project chat, node check-in/out for a member's registered desktop nodes. Not a compute node itself. Web app must be built mobile-responsive in v1 so v1.1 is a wrapper + push notifications, not a rebuild.

**Implications flagged:**
- No cash-out means $honey is a **closed-loop credit**, which simplifies tax/regulatory exposure; revisit only if the community demands it.
- Mobile in v1.1 pushes toward **React Native / Expo** sharing the React component package (D33) — one more reason to pick React over Svelte for the web/Tauri UI.

---

## Interview status

**Interview complete — 19 questions, 68 decisions.** Next steps (Loki):
1. Split this log into numbered ADRs (ADR-001+), one per architectural concern.
2. Scaffold the monorepo per the decisions above.
3. Register the project in Cmd Work and log the decisions there.
