# ADR-010: Node Desktop App ("OH Hive")

**Status:** Proposed · **Date:** 2026-09-04 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** ADR-000 Q1, Q4, Q8, Q12–Q14, Q17–Q18; D4, D11, D26, D27, D29, D42, D45–D48, D51, D61, D62, D66

## Context

A compute node is a member's own computer, contributed to the Hive at will (D4). Jack was explicit in Q8 that the node software is a proper desktop application — dock/taskbar icon, launcher, full GUI — not a headless daemon with a web dashboard. It shows check-in/out, schedule, hardware stats and earnings, and it is the place where the contributor makes the two trust decisions that shape what the scheduler may send them: internet access and tool level (Q13).

Q8 also fixed the structural rule that makes the self-healing network cheap: one core, two shells (D27). The regional server (`hive-server`) and the node app run the same Rust core crate — P2P networking, relay, job runner, agent runtime (D43). The desktop app wraps that core in a GUI and adds what a regional server must never carry: inference backends (D61) and a managed Python sidecar for the media ML stack (D62). Any node can therefore be promoted to relay when a regional server drops (D28) with no extra code path.

Q14 adds a duty the node app did not originally have: it is the return address for unfunded artifacts. When a project's storage funding lapses, artifacts are returned to the owner's node (D51), which means the app needs a "pending returns" inbox and must be online at some point in the grace period. Q17's all-modalities day-1 scope (D60) means the app also manages multi-gigabyte model weights and long-running ComfyUI workflows, with Windows + NVIDIA as a first-class target.

## Decision

1. **Product name and identifiers** (D64, D66). App name "OH Hive"; bundle/app ID `media.happyjack.ohhive`; Rust workspace `ohhive`; CLI binary `hive` (node core) alongside `hive-server` (regional, ADR-004). Both binaries are built from the same core crate.
2. **Shell: Tauri** (D29), with **Electron as the documented fallback** if Tauri platform quirks (tray behaviour, updater, WebView2 on Windows 10) block a release. The UI is React, consuming the shared component package from ADR-009 (D33). Fallback criteria: any P0 platform bug in Tauri open for more than one sprint on a supported OS.
3. **Platforms** (D11): macOS (Apple Silicon and Intel), Linux (x86_64, ARM64), Windows 10+ (x86_64). CUDA on Windows is a first-class CI test target, not best-effort (Q17).

   **Amended 2026-09-09 (ADR-018):** macOS Apple Silicon on macOS 27+ moved to a native SwiftUI shell over the shared Rust core (ADR-018), not this Tauri shell. **Intel Macs keep this Tauri shell as their permanent path** — confirmed by Jack 2026-09-09, not a temporary state pending Swift parity. macOS Apple Silicon on pre-27 systems also stays on Tauri until/unless that's revisited. Linux and Windows are unaffected by ADR-018 and continue exactly as decided here.

   **Amended 2026-09-09 (ADR-021), standing platform principle:** the ADR-018 call generalizes beyond just this one app — native Swift/SwiftUI is now the default for *any* current or future Apple-platform surface OH Hive builds (macOS, iOS, and whatever else Apple ships a first-party framework for), not a one-time exception for the desktop node app. Cross-platform tooling (React Native, Expo, Electron, etc.) is reserved for platforms Apple doesn't cover at all. First concrete consequence: ADR-012's v1.1 iOS mobile app is pulled forward and rebuilt as native Swift (ADR-021) instead of shipping as an Expo/React Native wrapper; Android, which has no native-Swift option, is unaffected.
4. **Dock/tray presence** (D26). The app runs as a menu-bar/tray resident with a main window. Closing the window does not check the node out; quitting does (with a confirmation if a lease is active, so the checkpoint is flushed first, D42).
5. **Architecture: core + backends + sidecar.**
   - The Rust core (`ohhive-core`) is linked into the Tauri process and exposes commands over Tauri IPC. No second daemon.
   - Inference backends implement the single `Backend` trait (D61: `capabilities()`, `run(job) -> stream`, `usage()`). v1 adapters: llama.cpp (all OSes), MLX (Apple Silicon), ComfyUI subprocess in API mode (image/video/music), whisper.cpp (STT), and a TTS adapter (Kokoro / XTTS / Piper, chosen per platform).
   - **Managed Python sidecar** (D62): the app bundles `uv` and creates a private venv under the app data directory for ComfyUI and Python-only models. The user's system Python is never used or modified. Sidecar lifecycle (install, upgrade, health, kill) is owned by the core; the sidecar is never present in `hive-server`.
   - Each adapter is independently enable-able so a modality can be disabled per node or per release (Q17 mitigation).
6. **Registration flow** (first launch, all steps required before the node can accept work):
   1. Sign in with the member's Supabase Auth identity (Apple/Google, D31); the app checks for an active `hive.hive_members` row.
   2. **Hardware probe**: CPU, RAM, GPU vendor/model, VRAM, disk free, measured upload/download bandwidth. Written to the node capability record.
   3. **Models**: choose which catalog models to pull (GGUF, ComfyUI checkpoints); weights are fetched from the nearest regional server cache first, Hugging Face second (Q17).
   4. **Modalities** offered: text/code, image, video, speech, music — derived from installed backends and hardware, user can untick.
   5. **Region**: derived from IP, editable (D58).
   6. **`allow_internet`**: whole-node flag, **default off** (D46). Copy must state plainly that on means member projects may reach the internet from this machine.
   7. **`tools_level`**: `inference_only` | `sandboxed_tools`, **default `sandboxed_tools`** (D48).
   8. **Storage offered** (only if the member enables "act as server for the Hive"): `storage_gb_offered`, `bandwidth_mbps` (Q14).
   9. **Schedule**: always-when-running, or a weekly time grid (D4).
   10. **Terms of Service acceptance**: the contributor ToS from ADR-011 (compute for $honey, no rights in outputs, no redistribution of `owner_only` material). Acceptance is recorded with version and timestamp on the node record.
7. **Check-in / check-out and schedule** (D4). Manual toggle in the tray menu and main window; scheduled windows check the node in and out automatically. Check-out with an active lease triggers a final checkpoint write before the lease is released (D42). The core heartbeats to the coordinator over the overlay, never to Supabase directly (D59).
8. **Earnings view** (D19, D23). Shows $honey earned by this node, broken down by card-lease segment (a resumed card credits each node for the segment it ran, Q12), tokens generated, compute-seconds for non-token modalities, and storage earnings if acting as server (D50). Balance is the wallet balance from the ledger, read via RLS; the node never writes ledger rows itself — the coordinator does, from hub-observed output (Q7).
9. **Pending-returns inbox** (D51, D52). Lists artifacts being returned to this member because project funding lapsed: artifact name, size, project, grace deadline, replica source. Actions: download to a chosen folder, or fund the project to cancel the return. Returned artifacts keep their content hash so resubmission deduplicates (D52). The app surfaces a badge on the tray icon while returns are pending.
10. **Preferences** with the following panes:
    - **Node**: check-in state, schedule grid, region, display name.
    - **Trust**: `allow_internet` and `tools_level` toggles, each with the plain-language consequence and the count of currently queued cards this node would become eligible for if changed (D47, D48).
    - **Backends & Models**: per-modality enable switches, installed models, disk usage, sidecar health, update/reinstall.
    - **Server role**: opt in to acting as a regional server / relay; `storage_gb_offered`, `bandwidth_mbps`, current replicas held.
    - **Earnings**: as in decision 8.
    - **About**: a quiet section stating that OH Hive is made by Happy Jack Media, with a link to *This Is Not A Draft*, the app version, core crate version, bundled backend versions, and the ToS version accepted. This About section is a house rule for every Happy Jack Media app and is part of the definition of done.
11. **Sandbox policy is enforced in the core, not the UI** (D45). Agent tools run in wasmtime with a network shim that is a no-op unless `allow_internet = true` *and* the card declared `requires_internet = true` (D47). The UI only displays the flags; it cannot widen them per card.
12. **Bootstrap.** The app fetches `hive.regional_servers where status = 'online'`, caches it locally, and falls back to the five day-1 hard-coded seeds (Q16). It holds a short-lived hub token minted by the coordinator rather than a write-capable Supabase JWT (Q16).
13. **Updates.** Tauri updater with signed releases; a node update never interrupts an active lease (defer until the lease is released or checkpointed).

## Consequences

### Positive
- One core crate means every fix to networking, leases or checkpointing lands in the regional server and the node app simultaneously; "any node can become a server" is free (D27).
- Trust decisions are captured once at registration with sane defaults (internet off, sandboxed tools on) and are visible in Preferences, which is the minimum for contributors running other members' agents on their own machines.
- Bundled `uv` venv isolates the media ML stack from the contributor's system, which is what makes "everything on day 1" survivable on Windows.
- Shared React package keeps the node app and web app visually and behaviourally aligned.

### Negative
- Tauri + Rust core + Python sidecar + ComfyUI is a heavy install (multiple GB with models). First-run time is dominated by weight downloads.
- Three OSes times several GPU stacks (Metal, CUDA, Vulkan, CPU) is a large test matrix; CI needs real hardware runners for at least macOS Apple Silicon and Windows NVIDIA.
- The pending-returns duty means an owner who is offline for the whole grace period loses the artifact from the Hive; the app can only warn, not prevent.

### Risks & mitigations
- **Tauri platform quirks** (tray, updater, WebView2 on Windows 10). Mitigation: Electron fallback with the same React UI and the core run as a child process; decide by the fallback criteria in decision 2.
- **Sidecar drift** (ComfyUI and model repos change fast). Mitigation: pin sidecar lockfiles per release; the adapter speaks ComfyUI's API, not its Python internals.
- **Long video jobs vs lease timeouts** (10–60 min, Q17). Mitigation: per-modality lease/heartbeat timeouts (ADR-005); ComfyUI checkpoint = workflow graph + completed node outputs.
- **Contributor misunderstanding of `allow_internet`.** Mitigation: default off; toggle copy reviewed by Jack; eligible-card count shown so the trade-off is visible.
- **Disk exhaustion** from model caches and held replicas. Mitigation: per-node disk budget in Preferences; the core refuses new weights/replicas past the budget.

## Open questions
- Which TTS engine ships per platform (Kokoro / XTTS / Piper)? (Default: Kokoro where it runs, Piper as the portable fallback.)
- Does the node app expose a local HTTP endpoint so the v1.1 mobile app can check the node in/out via the hub, or is remote check-in purely a coordinator command? (Default: coordinator command over the overlay; no local listener.)
- Should acting as a regional server from the desktop app require a minimum measured uptime before the node may hold sole replicas? (Default: yes; threshold set in ADR-007.)
- Is the hardware probe re-run on every launch or only on demand? (Default: quick probe every launch; bandwidth test on demand or weekly.)
- Storage-earning scaling for a node that donates both compute and storage (Q14, "TBD"). Deferred to ADR-002.

## Related
- ADR-002-honey-economics
- ADR-003-node-core-and-backends
- ADR-004-p2p-overlay-and-regional-servers
- ADR-005-scheduler-and-leases
- ADR-006-agent-runtime-and-sandbox
- ADR-007-artifact-storage
- ADR-008-auth-and-membership
- ADR-009-web-app
- ADR-011-ownership-and-licensing
- ADR-012-scope-and-roadmap
