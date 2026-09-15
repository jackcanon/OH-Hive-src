# ADR-034: Three subscription coordinators — ChatGPT, GitHub Copilot, Grok

**Status:** Accepted — order and scope confirmed by Jack 2026-09-15; implementation is P0 (Codex Stage 1 scaffold, partial) only. Everything below "Implementation contract" is still to be built.
**Date:** 2026-09-15
**Author:** Sif your friendly Codex Agent (design), formalized by Claude (Loki) from her provider survey and implementation-plan docs.
**Deciders:** Jack, confirmed directly 2026-09-15 (Cowork session, Loki).

## User requirement

Jack: "more people are likely to have a Claude login than a Claude API [key]... I want to make sure that users can bring their favorite cloud agent of choice." ADR-033 answers this for ChatGPT/Codex alone. Sif's `docs/SIF-SUBSCRIPTION-PROVIDER-INVESTIGATION-2026-09-15.md` surveyed the wider landscape (Copilot, Grok, Mistral, Gemini, Kimi, MiniMax, Z.AI, Qwen) and found genuine subscription-login or subscription-key routes for several. Claude itself is out of scope: Anthropic's Agent SDK terms permit third-party subscription auth only "unless previously approved," and Hive has no such approval (see `Halo-src/docs/CONTINUITY.md`, 2026-09-15).

## Decision

Build exactly three subscription coordinators, in this order, and stop: **ChatGPT (Codex) → GitHub Copilot → Grok.** Gemini, Mistral, Kimi, MiniMax, Z.AI and Alibaba/Qwen are explicitly out of this implementation queue — Sif's broader survey remains a reference for a future revisit, not an active build. Local models and BYOK API keys remain available regardless of this queue.

All three share one domain layer rather than three independent integrations: `subscription/service.rs`/`types.rs` (account handles, session lifecycle, typed events), one adapter per provider (`adapters/codex.rs`, `adapters/copilot.rs`, `adapters/grok.rs`), and shared `journal.rs`/`broker.rs`/`policy.rs` for recovery, tool authorization, exact placement and deduplication. Each adapter owns its own protocol/version/account logic; none of the three raw-model-substitutes inside `CodeBrain::next_turn`, and none reuses ADR-032's leased-parent path — each is an ADR-031 external coordinator session in its own right. See `docs/SIF-THREE-PROVIDER-IMPLEMENTATION-2026-09-15.md` for the full architecture, per-provider account/session mechanics, and the P0–P6 build order with acceptance gates.

GitHub Copilot's account connection (a Hive-owned GitHub App, device flow, OS-secure token storage) is the same credential layer ADR-036 (Git workspaces) needs for authenticated repo access — one GitHub connection, two consumers, not two competing logins.

## Consequences

Three new agent runtimes to supervise, three protocol adapters to maintain, three account-connection UIs (one tile each, Jack's stated order, never auto-selecting a coordinator). Each provider's subscription/quota behavior is real and enforced by that provider, not by Hive — no silent fallback to a paid API, no credit purchases, no pooling of one member's subscription for another's work. P0 (finishing Codex Stage 1's real acceptance: pinned schema generation, compiler/test verification) needs a real `codex` binary and a Rust toolchain, neither available in the sandbox that built the current scaffold — it needs a session with both before it can be called done.

## Related records

ADR-030 (submissions), ADR-031 (external adapter), ADR-032 (coordinator cards), ADR-033 (ChatGPT/Codex detail), ADR-036 (Git workspaces, shares the GitHub connection). `docs/SIF-CHATGPT-SUBSCRIPTION-INTEGRATION-HANDOFF-2026-09-15.md`, `docs/SIF-SUBSCRIPTION-PROVIDER-INVESTIGATION-2026-09-15.md`, `docs/SIF-THREE-PROVIDER-IMPLEMENTATION-2026-09-15.md`.

Claude (Loki), formalizing Sif's design and Jack's 2026-09-15 direction.
