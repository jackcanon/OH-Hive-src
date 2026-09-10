# ADR-003: Node Core and Inference Backends

**Status:** Proposed · **Date:** 2026-09-04 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** ADR-000 Q1, Q4, Q5, Q8, Q17 — D3, D11–D16, D27, D29, D60–D63

## Context

Hive ships two software distributions: an infrastructure-only regional server and a full node app that adds a compute contributor's inference capability (D3). The infra role must run on anything from a Raspberry Pi to an HP rack server, on ARM64 and x86_64 with low RAM. The compute role must run on macOS, Linux, and Windows 10+ from day one, and must be a proper desktop application with a dock/taskbar presence rather than a headless daemon. Jack also wants any node to be able to become a server for other nodes when a regional server drops, so the two distributions cannot be two codebases.

The original question of which LLM runtime to use ("whatever is most efficient") was reframed in Q5 into two meanings of "user-distributed model": distributed *capacity* (each node runs whole models it can fit; jobs are routed to capable nodes) versus distributed *inference* (one large model sharded across peers, Petals/exo style). Jack chose A first, B on the roadmap, so v1 is capacity-based and the architecture must leave a slot for sharded execution later without a schema migration.

Q17 then widened the v1 modality set to everything the film/audio/television/radio community makes: text, code, image, video, speech, and music, all on day 1. The media ML ecosystem is overwhelmingly Python (ComfyUI, diffusers, MusicGen, XTTS), which conflicts with the Pi-class footprint requirement for the server role. The resolution is to constrain Python to compute nodes only and to put every runtime behind one Rust trait so each modality can be enabled independently and the launch can be staged by modality if one path lags.

This ADR fixes the implementation language, the one-core-two-shells structure, the backend abstraction, the day-1 adapter list, and the capability model that the scheduler (ADR-005) consumes.

## Decision

1. **Rust for the core (D12, D27, D29).** The `hive` Cargo workspace produces the shared core library and two binaries: `hive` (node core, used by the desktop app) and `hive-server` (regional server, headless). Rust is chosen for static single-binary output on ARM64/x86_64, Tauri compatibility, and mature libp2p bindings. Go is the recorded alternative if Rust velocity becomes a blocker; the decision to switch would be a superseding ADR.
2. **One core, two shells (D27).** Crates: `hive-core` (P2P networking, relay, job runner, checkpointing, hub client), `hive-backends` (inference adapters, feature-gated per modality), `hive-server` (thin binary over core; no backends compiled in), `hive-node` (core + backends + local HTTP/IPC API consumed by the Tauri shell). A regional server is `hive-core` run headless; a node app is the same core plus backends plus GUI. Promoting a node to relay/server role is a runtime flag, not a different build.
3. **Server footprint budget (D12).** `hive-server` must build as a static binary (`musl` on Linux), run on a Raspberry Pi 4 with ≤ 256 MB RSS idle, and have no Python, no GPU, and no inference dependencies. `hive-backends` is excluded from the server build by Cargo features; CI asserts the server binary links nothing from it.
4. **v1 execution mode = distributed capacity (D14).** Each node runs complete models it can fit. `hive-core` has an `ExecutionMode` enum with `Whole` implemented and `Sharded` reserved (D15). The job schema carries `required_capabilities` (used now) and `shard_plan` (nullable, unused in v1). Capability records already include RAM, VRAM, and measured bandwidth so shard planning is possible later; exo is the leading candidate to evaluate for v2.
5. **The `Backend` trait (D61).**
   ```rust
   #[async_trait]
   pub trait Backend: Send + Sync {
       fn id(&self) -> BackendId;
       async fn capabilities(&self) -> Capabilities;          // modalities, models, limits
       async fn run(&self, job: JobSpec) -> Result<JobStream>; // streaming outputs + checkpoints
       async fn usage(&self) -> Usage;                          // tokens_in/out, compute_seconds, peak_vram
       async fn health(&self) -> Health;
   }
   ```
   `JobStream` yields `Delta::Text`, `Delta::Artifact{hash, mime}`, `Delta::Checkpoint(ptr)`, and `Delta::Done(Usage)`. `usage()` is the sole metering source for ADR-002; nodes report it, the coordinator validates it against the stream it received.
6. **Day-1 adapters (D16, D61).** Text/code: `llama.cpp` (GGUF; CPU, Metal, CUDA, Vulkan; all three OSes) as the universal engine, `MLX` as an optional accelerator on Apple Silicon. Image: Stable Diffusion / FLUX class via `ComfyUI` in API mode as a managed subprocess. Video: the same `ComfyUI` adapter with Wan / HunyuanVideo / LTX / AnimateDiff class workflow graphs. Speech: `whisper.cpp` for STT; TTS via Kokoro / XTTS / Piper, chosen per platform at build time. Music: MusicGen / Stable Audio Open / ACE-Step class models via ComfyUI or the Python sidecar. Ollama, vLLM, and exllama are not v1 adapters; Ollama may be wrapped later as a convenience.
7. **Managed Python sidecar, compute nodes only (D62).** Media adapters that need Python run in a venv created and owned by the node app using bundled `uv`, pinned by lockfile, isolated from the user's system Python. The sidecar exposes a local Unix socket / named pipe; `hive-backends` talks to it over that channel. `hive-server` never spawns it. Absence of a working sidecar disables the affected modalities in `capabilities()` rather than failing the node.
8. **ComfyUI as a subprocess, not a library.** The image/video/music adapter launches a pinned ComfyUI checkout in API mode inside the sidecar venv, submits workflow graphs over its HTTP API, and treats the workflow JSON plus completed-node outputs as the checkpoint unit. Custom nodes are installed from a Hive-curated manifest, not by the user.
9. **Per-modality capability advertisement (D63).** `Capabilities { modalities: set<Text|Code|Image|Video|Stt|Tts|Music>, models: [ModelRef{id, format, size_bytes, quant}], hw: {cpu_arch, ram_bytes, gpu: [{vendor, name, vram_bytes, api: Cuda|Metal|Vulkan|Rocm}]}, bandwidth_mbps, allow_internet, tools_level, schedule }`. Nodes recompute and publish this on startup, on model install, and on toggle change. The web app surfaces per-project eligible-node counts from it.
10. **OS matrix (D11).** Compute node: macOS 13+ (arm64, x86_64), Linux glibc 2.31+ (x86_64, arm64), Windows 10 1809+ (x86_64). Windows + NVIDIA/CUDA is a first-class test target because it is the dominant video-capable platform in the audience. Regional server: Linux (x86_64, arm64) primary, macOS secondary; Windows server unsupported in v1.
11. **Model acquisition.** Nodes pull weights by `ModelRef` from the nearest regional server acting as model cache/CDN (ADR-004, ADR-007); Hugging Face or other upstream is the origin the regional servers fill from, not something 2,000 nodes hit directly.
12. **Desktop shell (D29).** Tauri wraps `hive` on all three OSes; Electron is the fallback if Tauri platform quirks block launch. Shell details are ADR-010; this ADR only fixes that the shell is a thin client over the core's local API.

## Consequences

### Positive
- Rust static binaries satisfy both the Pi footprint and the Tauri shell with one language and one workspace.
- A single trait means each modality is a feature flag; a lagging video adapter does not block a text launch, and launch invites can be staged by modality.
- Capability records designed for v2 sharding avoid a migration when distributed inference lands.
- Reusing ComfyUI puts the audience's existing workflows and model zoo inside the Hive on day 1.
- Metering flows from one method (`usage()`), keeping the economics (ADR-002) independent of which engine ran the job.

### Negative
- Six modalities on day 1 is a large surface; each adapter has its own failure modes, model formats, and platform matrix.
- The Python sidecar reintroduces the dependency weight Rust was chosen to avoid, on compute nodes at least; installer size and first-run time grow accordingly.
- ComfyUI's API and custom-node ecosystem move fast; pinning implies a Hive-maintained fork or manifest and periodic re-validation.
- Rust learning curve and compile times may slow iteration compared with Go or TypeScript.

### Risks & mitigations
- **Scope overrun on media adapters.** Mitigation: text/code adapters are the launch gate; image/video/audio ship behind flags and are enabled per node as they pass conformance tests; invites staged by modality.
- **CUDA on Windows breakage.** Mitigation: dedicated Windows+NVIDIA CI runner; llama.cpp and ComfyUI both validated on it before each release.
- **Sidecar drift or corruption.** Mitigation: venv is content-addressed by lockfile hash; the node app rebuilds it on mismatch; `capabilities()` drops modalities whose sidecar health check fails.
- **Long-running video jobs vs lease timeouts.** Mitigation: per-modality lease TTLs and heartbeat cadence (ADR-005); ComfyUI checkpoints at each completed workflow node.
- **Metering divergence between `usage()` and the coordinator's received stream.** Mitigation: the coordinator's count is authoritative for token modalities; `usage()` is advisory and any node with persistent over-reporting is flagged.

## Open questions
- Go instead of Rust if velocity stalls? Default: Rust; revisit only via a superseding ADR after the first vertical slice.
- Which TTS engine per platform (Kokoro vs XTTS vs Piper)? Default: Piper on Pi-class/low-RAM nodes is moot (no TTS on servers); Kokoro on macOS/Windows, XTTS where a GPU is present. To validate.
- Music through ComfyUI or directly through the sidecar? Default: ComfyUI where a workflow exists (Stable Audio, ACE-Step), sidecar for MusicGen.
- Is MLX exposed as a separate backend id or as an acceleration path inside the text backend? Default: separate `BackendId::Mlx` so capability advertisement is honest about which engine will run.
- Minimum hardware floor for a compute node (e.g. 8 GB RAM)? Default: no hard floor; nodes below any model's requirement simply advertise no models.
- Does the node app bundle a default small text model so a fresh install can earn immediately? Default: yes, one ≤ 4 GB GGUF fetched on first run from a regional server.
- Where does distributed-inference (v2) shard coordination live — in `hive-core` or a separate crate wrapping exo? Open; only the schema slot is reserved now.

## Related
- ADR-001-hub-and-source-of-record.md — the coordinator is built from the same core crate.
- ADR-002-honey-economics.md — `usage()` as the metering source; compute-seconds for non-token modalities.
- ADR-004-p2p-overlay-and-regional-servers.md — libp2p in `hive-core`; regional servers as model cache.
- ADR-005-scheduler-and-leases.md — capability matching, per-modality lease TTLs.
- ADR-006-agent-runtime-and-sandbox.md — the agent loop runs inside the node core; WASM sandbox for tools.
- ADR-007-artifact-storage.md — `Delta::Artifact` hashes and model weight distribution.
- ADR-008-auth-and-membership.md — node registration produces the first capability record.
- ADR-009-web-app.md — eligible-node counts per project.
- ADR-010-node-desktop-app.md — Tauri shell, sidecar lifecycle UI, toggles.
- ADR-011-ownership-and-licensing.md — contributor ToS shown at node registration.
- ADR-012-scope-and-roadmap.md — v2 distributed inference, staged modality invites.
