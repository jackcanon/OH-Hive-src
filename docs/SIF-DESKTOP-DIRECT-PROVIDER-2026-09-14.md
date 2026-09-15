# Direct-provider desktop proposals (ADR-029)

Date: 2026-09-14
Author: Sif your friendly Codex Agent

## What is implemented

Opt-in Rust feature `desktop-provider` exposes `desktop::provider::AnthropicDesktop`. It takes a host-authorized PNG screenshot, task context, a local API key and an explicitly selected model, makes one direct HTTPS Messages API request, and returns a proposed `Request` or untrusted completion text plus token usage. No Hub, Supabase, Edge Function, coder/worker dispatch, native execution, UI or automatic tool-result loop is involved.

The provider adapter never calls `evaluate`, `FakeDesktop`, or an executor. The model supplies only action arguments. Session, call UUID, sequence, observation ID, policy revision and target are copied from the trusted turn into the returned request. The caller must submit that request to the existing broker gate with current authority, timing, focus and independently assessed risk. A missing risk classification is Unknown, never Routine. A returned completion is the model's claim, not proof of task success.

## Provider protocol

Pinned first profile: Anthropic `computer_20251124`, `anthropic-beta: computer-use-2025-11-24`, Messages API version `2023-06-01`. Explicit model choices are Sonnet 4.6, Opus 4.6 and Opus 4.5; no default or provider fallback. Anthropic's current documentation lists this earlier beta tool as supported for these models. Their newer `computer_toolset_20260801` has a different multi-tool response protocol and is intentionally not silently accepted by this adapter.

Source checked on 2026-09-14: https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool (Earlier tool versions and migration sections).

Only one computer tool is advertised. Accepted outputs: screenshot -> Observe, left_click with explicit integer coordinates -> Click, and nonempty type text -> Type. More than one tool call, alternate tools/toolsets, keyboard/scroll/drag/zoom actions, unknown action fields, malformed coordinates and truncated/refused responses are rejected. No unsupported action is silently skipped to extract an otherwise acceptable one. Host integration should surface that error rather than simulate a successful tool result.

Task and text limits: 16 KiB each. PNG: 4 MiB encoded, at most 1024 x 768; actual dimensions must match the observation. Validates allowed rectangle bounds and expired observations before transport. PNG is decoded with bounded allocation, verified through image completion, and re-encoded without metadata; palette transparency is expanded/preserved. Animated images are rejected. The adapter does not capture, crop, mask or rescale the screen. Native broker must supply an already-scoped canvas and map its coordinates to physical desktop coordinates consistently. This preserves a small canvas appropriate for the initial profile, not full-resolution desktop support.

Request budget: 1024 output tokens, 10-second connect timeout, 30-second request timeout, 256 KiB response cap checked both against advertised length and while streaming bytes. Usage is returned; session-wide spending/turn budgets are not implemented in this slice.

## Credentials and disclosure

`ApiKey::from_local_secret` accepts an explicitly injected secret. ApiKey and adapter have no Debug/Serialize implementation; auth header is marked sensitive. No automatic environment-variable lookup, key-file loading, credential persistence, logging or access to the real Halo key. Host code should retrieve a per-provider secret from its secure credential store and construct an adapter for the authorized session. This slice deliberately supplies no cross-platform disk secret store; keys remain process memory and are not guaranteed securely zeroized on drop.

A separate host-owned `upload_authorized` value is mandatory per turn. This is not a model-deserializable permission or a replacement for a native consent record. The broker owns scope/redaction, permission freshness, interruption and cancellation. Uploading an image is already disclosure even if a later proposed action is denied; the caller must authorize that disclosure first. After a response, action authority must be checked again because the observation/lease/grant may have expired while waiting. An upload already transmitted cannot be retracted by cancellation.

Transport is pinned to `https://api.anthropic.com/v1/messages`, TLS validation remains enabled, redirects and environment/system proxy routing are disabled. Reqwest's own retry policy is explicitly disabled as well as adapter-level retries. Provider errors expose only a static category or HTTP status; response bodies, URLs containing credentials, task data and image bytes are never included in errors. The internal mock transport seam is unavailable to external production callers.

## Calling shape

```rust
use hive_core::desktop::provider::{AnthropicDesktop, ApiKey, Model, Turn, Proposal};

// Inside trusted host code, after image-disclosure consent and secure key lookup:
let brain = AnthropicDesktop::new(ApiKey::from_local_secret(secret)?, Model::Sonnet46)?;
let proposal = brain.propose(Turn {
    task, png: authorized_png, observation: &observation,
    session, call_id, sequence, policy_revision,
    upload_authorized: true, now: broker_now,
}).await?;

// Caller must independently validate the returned request through its normal gate.
// Never treat receiving Proposal::Action as permission to execute it.
```

This is a single-turn proposal API. The host can supply a fresh task/progress description with the next authorized observation; there is no full Anthropic conversation/history or tool-result continuation manager here. No live provider smoke test was run, so real account/model availability and practical task quality remain unverified.

## Verification

On Midgaard/macOS:

- `cargo test -p hive-core --features desktop-provider --lib`: **31 passed, 0 failed**, proving this feature builds without Hub/Supabase dependencies enabled.
- `cargo test -p hive-core --features 'local-hub,sandbox,llama-cpp,desktop-provider' --lib`: **118 passed, 0 failed** against the current shared working tree.
- Nine new provider tests. Formatting and `git diff --check` passed. No Linux/Windows run, deployment or live provider validation.

The first full build exposed E0597 in Claude's new heartbeat test: SlowBackend returned a stream borrowing a stack-local MockBackend. Changed that test mock to a static lifetime in `local_hub/tests.rs`; production heartbeat/worker code was untouched. The passing full run includes that test.

 Tests use only synthetic images/keys and a mock HTTP transport; the production request builder is inspected without sending it. Tests cover exact tool profile, auth redaction, immutable binding, existing view-only policy denial, consent/expiry/image bounds, metadata removal, palette transparency, single-action parsing, unsupported/malicious action fields, batch/truncation/refusal errors, usage and completion, and no retries after transport/HTTP errors.

## Handoff to Claude

Review the pinned provider protocol and the host disclosure/authority seam. No changes to worker.rs, coder.rs, Swift, FFI or the native pilot. Future integration still needs secure-store/UI wiring, trusted capture/masking and coordinate mapping, cancellation and persistent grants, mixed-version capabilities, session budgets, a tool-result loop if desired, and live provider validation on explicitly approved synthetic input. Those are not advertised as complete by this adapter.
