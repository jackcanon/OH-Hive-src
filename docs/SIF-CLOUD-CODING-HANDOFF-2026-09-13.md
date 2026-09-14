# Cloud coding handoff — September 13, 2026

By Sif your friendly Codex Agent

Packages A and B from Claude/Loki's continuity-log handoff are implemented. The cloud endpoint and database RPCs are live. The web flow is built and browser-tested locally; it has not been published. Claude's worker release remains the final integration step.

Jack changed the live test provider to Nous during implementation because his direct Anthropic setup is having trouble. Nous is therefore supported alongside Anthropic and OpenAI. This extends the original provider enum; the response contract is unchanged.

## Claude: next actions

1. In your owned `crates/ohhive-core/src/worker.rs`, add `"nous"` to the existing `provider @ ("anthropic" | "openai")` CloudBrain branch and its supported-provider message. It still excluded Nous when Sif last inspected it. Sif did not edit worker.rs, coder.rs, or hub.rs.
2. Restore/commit the migration defining `hive.node_member_id(text)` and the phase-zero security migrations into this checkout. The helper exists live, but the file named in the handoff (`20260913150000_node_key_byok_management.sql`) is absent locally; current hub comments reference `20260913120000` instead. Resolve the filename/order rather than duplicating the helper. Sif's migration depends on that function.
3. Review the new files listed below, then release the web app together with the updated worker. Run one end-to-end code card on a disposable local repository, including actual file and command tools, and confirm the project-board result. Sif's real-provider test exercised the endpoint and tool-result conversation, not a live worker or filesystem changes.
4. Keep `model_id` absent on cloud code cards until you update the scheduler. Today's claim function treats it as a locally installed model constraint. The new UI/RPC use the member's saved cloud model preference via the Edge Function instead. Direct Edge requests may still specify `model`.

## Files and deployment

All paths below are relative to `Projects/Apps/OH Cloud-src`.

- `supabase/functions/code-brain-turn/{index,handler,protocol}.ts`: pure next-turn service. No local or remote tool execution, no logging of credentials/conversations/provider response bodies.
- `supabase/functions/code-brain-turn/protocol_test.ts`: 11 passing Deno tests.
- `supabase/migrations/20260913180000_code_session_create.sql`: applied to project `pxfbnuxcnerulbvbmowz`; migration history marked applied using the CLI. Only this migration was applied, not a broad push of the shared working tree.
- `supabase/tests/code_session_create.sql`: live rollback-only integration checks. Fixtures never commit and cannot be claimed by workers.
- `apps/web/app/code/new/page.tsx`: new member flow.
- `apps/web/app/fleet/page.tsx`: only Sif's import and link to `/code/new` added.

Final observed Edge deployment: `code-brain-turn` version **5**, ACTIVE, `verify_jwt: true`, matching `interview` version 16. No production web deployment, Git commit, or worker installation was performed by Sif.

## Edge contract

POST `/functions/v1/code-brain-turn`, with the same Supabase bearer/anon-key headers the current HubClient uses:

```json
{
  "raw_key": "node credential",
  "provider": "nous",
  "model": null,
  "messages": [
    {"role": "system", "content": "..."},
    {"role": "user", "content": "..."}
  ],
  "tools": [{"name": "read_file", "description": "...", "parameters": {"type": "object"}}]
}
```

Providers: `anthropic`, `openai`, `nous`. `model` may be omitted or null: **hub.rs serializes None as explicit null**, now covered by a regression test. BrainMessage/ToolSpec field names match Rust, including optional nullable content and tool_call_id, and omitted/empty tool_calls. Arguments are JSON objects.

Response is either:

```json
{"type":"text","text":"...","tokens_in":123,"tokens_out":45}
```

or:

```json
{"type":"tool_calls","calls":[{"id":"...","name":"read_file","arguments":{"path":"..."}}],"tokens_in":123,"tokens_out":45}
```

Authentication uses service-only `public.hive_admin_code_brain_member(p_raw_key)`, which calls the existing `hive.node_member_id` and requires active membership. This public wrapper is needed because the hive schema is not exposed through PostgREST. It is VOLATILE because key verification records last use. The migration also reloads the API schema cache. Invalid/revoked/unowned or inactive-member keys fail before provider access. Credentials come from `hive_admin_member_key`; preferred models come from `hive_admin_member_models`. No provider key reaches the node.

Model selection: request override, then saved member preference, then `CODE_BRAIN_<PROVIDER>_MODEL`, existing interview provider environment setting, or built-in default. Defaults: Anthropic `claude-sonnet-4-5`, OpenAI `gpt-5`, Nous `anthropic/claude-sonnet-4.6`. Nous uses the existing interview endpoint `https://inference-api.nousresearch.com/v1/chat/completions`, OpenAI tool protocol and `max_tokens`; OpenAI uses `max_completion_tokens`. Anthropic uses `/v1/messages`, separate system text and grouped tool results.

One provider request per invocation, 90-second request timeout, 4,096 output-token cap, 2 MiB request-body cap, up to 1,000 messages/32 tools. The function does not retry or silently switch providers. Complete tool batches are required: all results for the previous assistant turn must appear before the next turn. Anthropic receives them as one user message. Unknown tools, duplicate IDs, missing results, malformed JSON arguments, empty answers, refusals/truncated turns and invalid usage fail closed.

Errors are fixed codes in `{ "error": "..." }` with HTTP status. Examples: 401 invalid node, 409 missing provider key, 409 Anthropic workspace-scoped key required, 402 exhausted credits/quota, 429 provider rate limit, 502 invalid/rejected provider response, 503 database lookup unavailable, 504 provider unreachable/timeout. Arbitrary upstream text is discarded, never echoed or logged.

## Session creation

`hive_code_session_projects()` returns only the signed-in active member's owned, undeleted local-mode projects. It is not a general project listing and does not expose other owners' projects.

`hive_code_session_create(p_project_id, p_task, p_workspace_path, p_repo_url, p_repo_ref, p_brain, p_model_id, p_max_turns, p_cloud_consent, p_request_id)` is security-definer, authenticated-only, membership-checked, and owner/local-mode checked under a project-row lock. It validates task, absolute Unix/Windows path OR repository, ref, provider, configured key, explicit cloud consent, and 1–100 turns. Cloud cards reject an explicit model_id to avoid the existing claim filter; use Settings instead. HTTPS and conventional `git@host:path` repositories are supported; credential-bearing HTTPS URLs are rejected.

The generated ready card has modality `code`, matching CodeSessionSpec fields, plus **`tools_level: "sandboxed_tools"`** required by the actual scheduler gate. Cloud/repository jobs require internet. A supplied request UUID makes retries idempotent; using the same UUID with different card capabilities is rejected. No Honey is charged by these new RPCs or the Edge Function.

The form provides local and configured Anthropic/OpenAI/Nous choices, model preference display, cloud-context consent, and a queued-state link to the project board. It does not promise a specific destination computer. Existing-folder tasks require that path on whichever eligible own-fleet node claims the job; repository cloning uses that computer's existing Git authentication. Commands use the worker's operating-system permissions; the legacy `sandboxed_tools` flag is not a new OS security sandbox. The turn cap is not a spending cap.

## Verification evidence

- `deno test supabase/functions/code-brain-turn/protocol_test.ts`: **11 passed**. Includes two-tool Anthropic grouping, OpenAI serialization, Nous routing/preference, malformed/orphan/duplicate tool messages, unknown tools/truncation, missing key/node rejection, one-request semantics, safe errors/body limit and Rust null model.
- `supabase db query --linked --project-ref pxfbnuxcnerulbvbmowz --file supabase/tests/code_session_create.sql`: passed live after Nous addition. Checks ownership, local-mode gate, absolute Unix/Windows paths, consent, max turns, contract fields, idempotency/conflicts and grants. All fixtures rolled back.
- `pnpm --dir apps/web build`: passed compilation, lint/type validation and generation of `/code/new` after Nous changes. Existing Supabase Node-version deprecation warnings remain.
- Browser: sign-in boundary, empty/error state, Fleet-to-form navigation, configured-only Nous option, consent-disabled submit, and queued state all inspected. Synthetic in-browser RPC responses were used for the member flow; one submission carried `p_brain: "nous"`, `p_model_id: null`, consent true, expected task/path and a request UUID. No live code card was created. No framework overlay or browser exception was reported in the final fixture flow.
- **Real member/node + Nous** on the final deployment: first response HTTP 200 `tool_calls`, exactly two `read_file` calls for synthetic `a.txt`/`b.txt`; both tool results were appended in one follow-up request; response HTTP 200 `text`, **“Both files received.”** Final turn usage 793 input / 7 output tokens. Repeated successfully with explicit `model: null` matching Rust. Tools were simulated; no files were read/executed by the provider test.
- Live negative checks: OpenAI without a configured key → HTTP 409 `provider_key_not_configured`; invalid node → HTTP 401 `invalid_or_revoked_node_key`.
- Direct Anthropic was attempted but its stored API key requires `anthropic-workspace-id`; Jack directed Sif to use Nous instead. No direct Anthropic successful exchange or real OpenAI inference is claimed. Anthropic's multi-result translation and OpenAI behavior are covered by tests.

## Remaining work beyond this slice

Worker/Nous dispatch and an actual local end-to-end job; coordinated web/worker release; restore missing migration sources; machine/workspace-aware scheduling; durable pause/resume/checkpoints and private/community policy; provider spending controls. This slice supplies a tested cloud brain and creation form, not the full fleet coordinator from the earlier recommendations.

Reference protocol docs reviewed: [Anthropic tool-result handling](https://platform.claude.com/docs/en/agents-and-tools/tool-use/handle-tool-calls), [OpenAI Chat API](https://developers.openai.com/api/reference/cli/resources/chat). Nous wiring follows the existing Hive interview implementation and was verified against the live service; its public docs page could not be opened by the web tool in this session.

Signed: Sif your friendly Codex Agent
