# Desktop session controls and fake-provider integration

2026-09-15 UTC — Sif your friendly Codex Agent

## Completed scope

Added `desktop/limits.rs`, `desktop/session.rs` and tests, plus a budgeted entry point in the existing provider adapter. All changes are within `crates/ohhive-core/src/desktop/`. No worker/coder, Swift/FFI, native execution or live provider calls.

`DesktopProfile { name: "desktop", version: 1 }` is the pure profile contract. Validation rejects other names/versions; strict deserialization rejects missing fields and extra capabilities. A new DesktopSession requires this profile and a non-nil session ID. The existing `PROFILE = "desktop_v1"` constant remains untouched; future claim wiring must explicitly construct/validate the structured contract, not assume every string is compatible.

`DesktopSession` privately owns a FakeDesktop and Budget. `execute_step` checks the session, batch ordering and session limits, then calls the existing fake executor with current authority, policy, observation, independently classified risk and per-request approval. A Batch contains only the ordered call IDs, never blanket consent. It is processed one step at a time so trusted focus, revocation, interruption and uncertainty events can arrive between actions. Every non-success outcome (including confirmation needed) permanently stops that batch. Continuing after review requires a new batch; existing call-ID replay protection still applies. Session interruption/uncertainty is terminal in this wrapper; it has no auto-resume method.

## Limits and accounting

Configurable pilot defaults (engineering choices, not measured promises):

- 100 provider turns.
- 900,000 milliseconds / 15 minutes per session.
- 128 MiB cumulative budgeted provider wire bytes plus serialized action-request bytes.
- 1,000 input-action attempts, maximum four per rolling 1,000 milliseconds.
- 1,000,000 micro-USD / $1 reserved or accounted provider cost.

Provider requests reserve serialized JSON body size plus the adapter's full 256 KiB response allowance. This is deliberately conservative, not exact wire/HTTP-header accounting. Rejected action attempts can consume bytes/action attempts; these counters never refund action attempts. Screenshot/Observe requests undergo time/byte/pending-call checks but do not consume the input-action rate quota. No native image capture is performed or costed here.

Clock input is trusted monotonic milliseconds for budgets. Existing authority/observation deadlines keep their existing broker time domain (`execute_step` takes both `now` and `now_ms`). Backwards milliseconds and exact-deadline arrivals fail closed. Host integration must supply consistent clocks and cannot let a model choose them.

`AnthropicDesktop::propose_in_session(turn, session, now_ms, SpendQuote)` reserves a turn, bytes and maximum cost before any HTTP call. The quote is host-trusted, never model-generated. In conservative mode, full quoted cost remains charged even on a successful response. With trusted token prices, returned usage is converted to integer micro-USD, rounded up, and unused reservation is refunded. Errors/malformed responses or unavailable arithmetic retain the full reservation. If actual computed cost exceeds the quote, the ledger records the overrun, permanently stops, and returns no proposal. Cancelled futures leave the reservation unresolved; subsequent calls and batch actions are blocked rather than treating cancellation as free.

**Billing guarantee is conditional:** a strict cap requires a valid pre-call upper bound covering every billable token/feature and correct provider pricing. Post-response usage cannot undo an already billed overrun. This slice does not fetch prices, call a token-count endpoint or estimate a verified input-token ceiling. A host without a trustworthy upper bound must not present this as an unconditional provider invoice cap. No production rate table was invented.

## Integration boundary

The provider still only proposes actions; it never calls evaluate or execution. The session wrapper is the separate caller responsible for fake dispatch. Low-level `AnthropicDesktop::propose` and `FakeDesktop::execute` remain available as pre-existing primitives; future production wiring must use the budgeted session path. This is not yet enforcement in worker claim/dispatch.

Hold one session/ledger for each logical session. There is no session-budget restart serialization, persistent accounting store, extension/reset API, multi-process budget arbiter or global active-desktop-session registry in this simulation slice. Do not reconstruct an old session ID with fresh limits after restart. A production resume must restore trusted accounting or remain stopped. The existing low-level journal recovery remains unchanged and does not itself restore these new budgets. Persisted accounting/native integration is a prerequisite for claiming restart-safe budget enforcement.

Per-action consent continues to bind exact request/risk/expiry; approvals for one action cannot authorize another. Explicit grant revocation blocks later steps even if every step had its own approval. Focus changes still invalidate observations using the existing trusted high-water-mark API. A failed batch never dispatches the remaining calls.

## Verification

- `cargo test -p hive-core --features desktop-provider --lib`: **42 passed, 0 failed**.
- `cargo test -p hive-core --features 'local-hub,sandbox,llama-cpp,desktop-provider' --lib`: **129 passed, 0 failed**, including current ordinary-coding regressions.
- 11 new tests: atomic reservations, exact budget boundaries, uncertain charges, quote overrun, backwards time/deadline, rolling action-rate boundary, unsupported profile fixtures, separate consent and blanket-consent rejection, revoked/interrupted/uncertain batches, ordering and observation freshness, action byte limits, unresolved-provider dispatch blocking, multimodal mocked-provider-to-FakeDesktop flow, failed calls and usage-based pricing settlement.
- Only synthetic PNGs, keys and provider responses. No live network calls to Anthropic or native desktop actions. Full suite includes existing local listener tests. Midgaard/macOS only; no Linux/Windows run.
- Formatting and `git diff --check` passed.

## Claude handoff

Your queued pure contract/simulated session pieces are implemented and tested. Review the structured profile and required budgeted entry point before integrating the shared executor/claim path. Bind upload consent, current authority and both clocks at the trusted host. Own the one-active-session registry and durable budget restore alongside your actual executor wiring. The full ADR-029 phase-1 integration gate remains open until that worker/coder work lands; these tests do not advertise real desktop capability or complete the native pilot.
