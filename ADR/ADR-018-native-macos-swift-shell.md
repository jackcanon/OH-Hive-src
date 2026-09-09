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

## Consequences

- Two UI codebases to keep in feature parity going forward (SwiftUI for macOS, React/Tauri for Windows/Linux) — real, ongoing cost, accepted for the sake of deep OS integration on the platform where most of the team actually works.
- The UniFFI bridge is new infrastructure to build and maintain; it pays for itself by keeping crypto/networking/scheduling logic in exactly one implementation.
- Xcode's native code-signing/notarization flow (`xcodebuild -exportArchive` + `notarytool`) replaces the hand-rolled Tauri signing steps in CI for macOS once phase 2 lands — likely simpler than what the Tauri pipeline needed, since it's Apple's own standard path for a native app.
- Local mode can offer a genuinely free, private, no-download inference option on macOS 27+ via Foundation Models — something the Tauri/Ollama path can't match on any platform.

## Open questions

- Exact `uniffi` version / proc-macro API surface to pin (crate is new to this repo).
- Whether phase 2 (Setup/Server/Tunnel) also moves to Swift, or those roles stay reachable only via the CLI (`hive server`) on Mac once the GUI is chat-first.
- Minimum macOS version to target — Foundation Models needs a recent OS; older Macs may need to fall back to Ollama-only with no Apple Intelligence path.

## Related
- ADR-015 (desktop app as local AI workstation — this ADR is macOS's implementation of it)
- ADR-010 (original Tauri desktop shell decision, amended for macOS by this ADR)
- ADR-004 (P2P overlay/coordinator protocol — unchanged, shared via the Rust core)
