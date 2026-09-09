# ADR-018 — Native macOS Shell: Swift/SwiftUI Over a Shared Rust Core (UniFFI)

Status: Proposed
Date: 2026-09-08

## Context

ADR-015 committed to the desktop app becoming a real chat-first local AI workstation — "the go-to experience for AI on a user's machine," pairing-as-compute-node reduced to one panel among several. That ADR assumed the existing shell: Tauri (Rust core + a React/TypeScript WebView UI), the same stack the web app uses, cross-compiled to macOS, Windows and Linux from one UI codebase.

Two things changed the calculus for macOS specifically, both raised by Jack:

1. **Apple's Foundation Models framework (macOS/appleOS 27, WWDC 2026)** now exposes Apple's on-device model — rebuilt, with vision input added this cycle — through a single native Swift API with built-in tool-calling, alongside a new Core AI framework replacing Core ML, and a terminal `fm` CLI. None of this is reachable from a WebView; it is Swift-only. If OH Hive wants the local execution engine ADR-015 calls for to have the option of running against Apple's own on-device model (private, no download, tool-calling already solved), that path only exists in Swift.
2. Jack wants the Mac app to feel like a real Mac app and pick up new macOS platform features as they ship, not wait on Tauri/WebView support for them.

The counter-consideration — cross-platform reach — still matters for Windows and Linux, where Tauri stays exactly as-is. Nothing here proposes touching those builds.

## Decision

1. **Split the shell by platform.** Windows and Linux keep the existing Tauri (Rust + React) app unchanged. macOS gets a new, separate native SwiftUI app. This repo now ships two desktop UIs, not one — an accepted, explicit cost, not an oversight.
2. **One shared Rust core, not two implementations of the sensitive logic.** `ohhive-core` (capabilities, the hub RPC client, the worker state machine, the wasmtime sandbox) and `hive-server` are not reimplemented in Swift. Crypto, the P2P/coordinator protocol (ADR-004), ledger math, and node scheduling stay in one place, exercised identically by both shells.
3. **Bridge via UniFFI**, Mozilla's Rust↔foreign-language binding generator (the same pattern used by Firefox and other security-sensitive cross-platform apps). A new crate, `crates/ohhive-ffi`, wraps the subset of `ohhive-core`/`hive-server` the desktop UI needs behind `#[uniffi::export]` functions and a callback interface for async events (activity log lines, worker/server state changes) — no hand-maintained `.udl` file, using UniFFI's proc-macro mode. `uniffi-bindgen` then generates the Swift wrapper module from that crate, built for `aarch64-apple-darwin`.
4. **Phase 1 scope** (this pass): pairing (`pair_begin`/`pair_cancel`), the compute-node worker (`worker_start`/`worker_stop`), `snapshot`/`about`, config (`set_config`), and the activity/event stream — enough for a real Node + About experience natively. **Deferred to phase 2**: first-run Setup (hardware assessment, Ollama install, model ladder), the regional-server role, and Cloudflare Tunnel setup — all real, all more surface area, not worth blocking phase 1 on.
5. **The chat/agent engine from ADR-015** (the local, unsandboxed multi-turn loop) is a macOS-first feature under this ADR: it gets built as native Swift calling Apple's Foundation Models framework directly for tool-calling and on-device inference, with Ollama/llama.cpp as a fallback backend via the same Rust core path other cards use. Windows/Linux get that engine on their own timeline, most likely calling out to an existing agent SDK per the ADR-015 open question — not blocked on this decision.
6. **Release pipeline**: the `.github/workflows/release.yml` `desktop-macos` job (Tauri build + codesign + notarize, just stood up) is retired once the Swift app replaces it — not deleted yet, since phase 1 doesn't yet cover parity (Setup/Server/Tunnel). Runs in parallel with the Tauri macOS build until phase 2 lands, then the Tauri macOS job is dropped and only Windows/Linux keep using Tauri.

## Amendment (2026-09-09) — macOS 27 (Golden Gate) feature integration

Loki ran a research pass on what macOS 27's rebuilt Foundation Models framework and related platform features give this Swift shell that a WebView can never reach (`docs/oh-hive-macos27-feature-map.md`, Cmd Work OH Hive project, 2026-09-09). Jack reviewed the recommendations and made the calls this ADR needed; the following amends decisions 4–6 and the open questions above.

7. **ADR-015's local chat/agent engine (decision 5) is built against `LanguageModelSession` only**, using `DynamicProfile` to route a turn across `SystemLanguageModel` (on-device, quick/private/offline), `PrivateCloudComputeLanguageModel` (`.light`/`.deep` reasoning for planning-heavy turns), and Anthropic's `ClaudeLanguageModel` first-party package (bring-your-own-key, read via a Keychain token provider — never stored in `nodeconfig`). No parallel Ollama/HTTP code path is built inside the Swift engine itself; Ollama/llama.cpp stay reachable exactly as today, through the shared Rust core, for card execution. Session history stays on-device, indexed in Core Spotlight, with the Spotlight search tool wired into the session for local RAG.
8. **Phase 1 (this pass) additionally ships**, alongside the UniFFI bridge: App Intents + App Schemas (`StartHiveNode`, `StopHiveNode`, `NodeStatus`, `EarningsToday`, `SetTrustLevel`, with `HiveProject`/`HiveCard` modeled as `IndexedEntity`, tested via `AppIntentsTesting`); a WidgetKit widget showing node state and Honey earned today; native SwiftUI 27 materials for the visual language. Deployment target for the Swift app is macOS 27, Apple Silicon (`arm64`) only — this was already implied by decision 1/3 but is now explicit.
9. **Phase 2 additionally ships**: a Core AI / `MLXLanguageModel` backend as the on-macOS-27 replacement for the Ollama-install step in Setup (`ohhive-core::setup::install_ollama`) — the model ladder offers `.aiasset` bundles pulled from the regional-server cache instead of (or alongside) Ollama pulls; `fm serve`'s local OpenAI-compatible endpoint is the no-bridge interim the Rust core can hit over plain HTTP before a dedicated adapter exists. A new `HiveLanguageModel` SPM package, conforming to `LanguageModel`/`LanguageModelExecutor`, submits cards to the Hive and streams results back into a session. Foundation Models' transcript entry types become the job wire format referenced from `packages/schema`, rather than inventing a parallel shape.
10. **CI adds Evaluations-framework tests plus the Foundation Models Instruments profile** for the interviewer and card-verification paths, once the local engine (decision 7) exists to test.
11. **Guardrail, effective immediately for any code touching Foundation Models**: Apple's on-device (`SystemLanguageModel`) and Private Cloud Compute models may only serve `execution_mode='local'` runs — the node owner's own cards, run on their own machine, per ADR-018 decision 5/ADR-015. They must never be scheduled against another member's paid, Hive-distributed card. Hive-distributed work continues to run on open-weight models (Ollama/llama.cpp today, Core AI/MLX in phase 2) via the shared Rust core. This preserves the ToS/licensing boundary in ADR-011 and avoids routing another member's paid work through a per-seat Apple entitlement.

**Decisions on the three items flagged as Jack's call:**
- **PCC entitlement reachability**: assume the `com.apple.developer.private-cloud-compute` entitlement is reachable from the Developer ID DMG distribution (not App Store-only) for planning purposes; this gets verified for real once phase 2's Core AI work starts, and this ADR is amended again if that assumption turns out wrong.
- **Cached/reasoning token Honey rate**: not decided yet. `response.usage`'s cached/reasoning token counts are logged from day one wherever they're available, but ADR-002's rate table is not amended until there's real usage data to price against. Tracked as an open question below, not a blocker.
- **Intel Macs**: keep the Tauri app as their permanent path (see ADR-010 amendment below); the Swift/UniFFI shell in this ADR is Apple Silicon only, on macOS 27+.

## Consequences

- Two UI codebases to keep in feature parity going forward (SwiftUI for macOS, React/Tauri for Windows/Linux) — real, ongoing cost, accepted for the sake of deep OS integration on the platform where most of the team actually works.
- The UniFFI bridge is new infrastructure to build and maintain; it pays for itself by keeping crypto/networking/scheduling logic in exactly one implementation.
- Xcode's native code-signing/notarization flow (`xcodebuild -exportArchive` + `notarytool`) replaces the hand-rolled Tauri signing steps in CI for macOS once phase 2 lands — likely simpler than what the Tauri pipeline needed, since it's Apple's own standard path for a native app.
- Local mode can offer a genuinely free, private, no-download inference option on macOS 27+ via Foundation Models — something the Tauri/Ollama path can't match on any platform.

## Open questions

- ~~Exact `uniffi` version / proc-macro API surface to pin~~ — resolved: UniFFI 0.28, proc-macro mode (`uniffi::setup_scaffolding!()`, no `.udl`), confirmed working end-to-end against real generated Swift bindings.
- ~~Whether phase 2 (Setup/Server/Tunnel) also moves to Swift~~ — resolved: yes, all three move to Swift (Setup shipped 2026-09-08; Server and Tunnel tracked as follow-on work).
- ~~Minimum macOS version~~ — resolved by the 2026-09-09 amendment: macOS 27, Apple Silicon only. Older/Intel Macs use the Tauri app (see ADR-010 amendment).
- **How cached/reasoning tokens earn Honey in the ADR-002 rate table** — explicitly deferred by Jack (2026-09-09) until real `response.usage` data exists to price against.
- Whether the `com.apple.developer.private-cloud-compute` entitlement is actually grantable to a Developer ID (non-App-Store) app — assumed yes for planning (2026-09-09); needs real verification before phase 2's Core AI work ships.

## Related
- ADR-015 (desktop app as local AI workstation — this ADR is macOS's implementation of it)
- ADR-010 (original Tauri desktop shell decision, amended for macOS by this ADR)
- ADR-004 (P2P overlay/coordinator protocol — unchanged, shared via the Rust core)
