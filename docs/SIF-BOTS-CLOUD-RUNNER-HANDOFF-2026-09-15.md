# S-A: bounded Claude / Nous replies

Implemented by Sif; executor integration belongs to Loki. This is a tool-free reply component, not a deployed end-to-end feature.

## Integration

`bots::CloudTurnRunner::new(hub_url, anon_key, raw_node_key, authenticated_owner)` is exported with `bots` + `hub` (also included by `local-hub`). Wrap it in `Arc<dyn LocalBotsTurnRunner>` and select it only for `AnthropicByok` / `NousByok`. The constructor accepts a trusted HTTPS hub origin, not an arbitrary agent endpoint. The owner must come from authenticated configuration. Node key is sensitive and must never be logged.

The runner independently rejects Local/subscription runtimes, archived profiles, owner mismatch, wrong-conversation history, and oversized context before networking. No local model or execution-capacity slot is required. Context uses supplied speaker display names, includes the incoming message once, and excludes attachments. Current role/policy references are not expanded into executable tools or custom instructions.

Loki's remaining pieces:

- Add the optional cloud runner to the executor and update its Local-only trait comment.
- Update route notices and shell preflights: supported cloud agents must not be labeled unsupported or blocked by missing local models. Wire construction on CLI, FFI and Tauri through their trusted account context; coordinate Sif-owned shell edits.
- Respect primary routing/claim ownership so multiple hosts cannot independently execute a turn.
- Preserve explicit cloud intent: local-only conversations/jobs must never take this route.

## Server contract

`POST /functions/v1/bots-turn` accepts `owner_id`, `provider` (`anthropic` or `nous`), `agent_name`, `participants_note`, `messages: [{speaker,text}]`, optional `raw_key`. Native calls send the hub anon key in Authorization/apikey and raw node key in the body. Browser callers can use a real member JWT without raw_key.

The server verifies the supplied node credential (or member JWT), derives the member itself, verifies active membership, and compares the claimed owner before looking up keys. It resolves only that member's selected provider key and saved model through existing service-role RPCs. No provider key reaches a device. No hub-funded key, cross-provider fallback, tools, persistent memory mutation or background model request.

Response: `{reply_body, usage: {prompt_tokens, completion_tokens}}`. HTTP 429 maps to `NoCapacity`; authorization/configuration/provider failures map to sanitized `RuntimeFailed`. No response/provider body is included in error messages or logs. Errors use `cache-control: no-store`.

Bounds: 65 messages (64 history plus incoming), 64 KiB combined message text, 512-byte names, 8 KiB roster, 128 KiB encoded request, 64 KiB reply, 256 KiB provider/Rust response envelope, max_tokens 2048. Fixed provider endpoints, redirects prohibited. Server RPC deadline 10 seconds per call, provider deadline 90 seconds, client HTTP deadline 110 seconds, outer runner deadline 115 seconds. Cancellation drops the client request; it cannot guarantee an already-started provider inference or charge is cancelled.

## Deployment and acceptance still required

Deploy `bots-turn` using the project's normal Supabase Edge Function process. Existing SUPABASE_URL / SUPABASE_SERVICE_ROLE_KEY environment and service RPCs are required. Default models follow the existing interview path: Claude `claude-sonnet-4-5`, Nous `anthropic/claude-sonnet-4.6`. Override via BOTS_ANTHROPIC_MODEL / BOTS_NOUS_MODEL; saved member model preference wins. These are inherited defaults, not a newly verified model catalog. Confirm current account availability at deployment.

Gateway authentication must allow the same anon-key native entry path as `interview`; if deployed with gateway JWT verification disabled, this function's own authentication remains mandatory. Do not expose its service-role credential. No new database migration.

Private-only enrollment identities are not accepted by these community-node/member RPCs. They require a separately designed private identity-to-BYOK binding; do not substitute another member or use a community key as a bypass.

There is no durable provider-response idempotency record. Delivery claims prevent ordinary concurrent duplicate execution, but a crash after provider completion can incur another charge on retry. No automatic retries inside either component. Do not claim exactly-once billing. Global/per-member spending caps remain a separate feature; existing delivery loop limits and explicit automation controls must remain in force.

Verify after integration/deployment with one explicit Nous message in a room and a DM, one Claude message when that account is repaired, owner mismatch and revoked-node rejection, provider 429 requeue, and a local-only message with zero Supabase calls. No live paid provider requests were made for this implementation.

## Verification

Deno handler suite: 8 tests pass. Entry point type-check passes with the actual Supabase SDK. Rust `bots,local-hub` suite: 157 library tests (including six cloud-runner tests), 6 route-notice tests and 10 loop-safety tests pass. Tests cover named context, wrong owner/runtime/conversation rejection, unsafe URLs, response limits, real loopback 429/error/success responses, timeout and pre-cancellation. Provider calls are mocked; no live provider acceptance claimed.

A separate `--features bots` build fails because the existing unconditional executor module imports feature-gated local_hub. Confirmed that unconditional export exists in HEAD; unrelated to this runner. Loki should repair the feature relationship or gate the executor. Supported tested combination here is `bots,local-hub`.
