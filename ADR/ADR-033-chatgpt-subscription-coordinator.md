# ADR-033: ChatGPT subscription coordinator through a local Codex runtime

**Status:** Proposed — implementation specified, not built.  
**Date:** 2026-09-15  
**Author:** Sif your friendly Codex Agent  
**Deciders:** Jack and Claude (Loki).

## User requirement

Jack wants people to use their existing subscription instead of arranging separate API billing. Keep the private fleet topology: six computers with independent local models collaborating through Hive. Prefer subscription access for new cloud onboarding where supported; retain local and explicitly selected API options. Never silently switch to paid API use when a subscription limit is reached.

## Proposed decision

Integrate Codex app-server as a supervised per-user local process over stdio, with managed ChatGPT browser/device sign-in. Add a scoped Hive MCP tool bridge for durable local-worker job submission, status, results and coordinator watches. Keep the runtime credential lifecycle isolated from Hive's hub credentials and the user's separate Codex installation. Model/account limits are discovered through the runtime. Subscription entitlement is not a general OpenAI API key or a universal no-charge guarantee.

The desktop runtime session is an external coordinator under ADR-031, with its own durable journal and linked cards. It does not impersonate an ADR-032 leased parent, occupy a worker slot, or run inside the existing per-turn CodeBrain interface. Preserve that older path. Require exact workspace/target binding, server-side deduplication and authorization before automatic delegation. Keep one model per worker; no shared-model-server conversion.

## Implementation contract

Claude: use the complete [implementation handoff](../docs/SIF-CHATGPT-SUBSCRIPTION-INTEGRATION-HANDOFF-2026-09-15.md). It specifies module seams, process packaging, versioned JSON-RPC flow, MCP bridge configuration, tool contracts, durable journals, failure recovery, UI/FFI, subscription-limit handling and six staged delivery gates with acceptance tests.

The handoff includes an offline schema check of Codex 0.149.0. Latest documentation differs from that schema; do not assume externally injected tokens or dynamic tools are supported. Use managed login and configured MCP. No app implementation, live authentication or deployment occurred in drafting this ADR.

## Consequences and release boundaries

Users can potentially access cloud coordination through account entitlements without supplying an API key. Hive must maintain an additional agent runtime and protocol adapter. Losing cloud access pauses coordination while existing local cards can continue. Login/limits and Hive membership remain separate. App quit initially suspends coordination; restart recovers it. Background/headless service behavior and web access require separate verified integration gates.

Official app-server docs contain experimental/production-support caveats, especially for remote/WebSocket paths; validate the selected release and account/deployment support before shipping. Enterprise clients require the documented OpenAI registration follow-up. No unsupported-subscription workaround, automatic purchasing or API fallback is part of this design.

## Related records

ADR-022 private fleet; ADR-024 coding workers; ADR-030 submissions; ADR-031 external adapter/placement; ADR-032 coordinator cards. [Official app-server documentation](https://learn.chatgpt.com/docs/app-server), [authentication](https://learn.chatgpt.com/docs/auth), [configuration](https://learn.chatgpt.com/docs/config-file/config-reference).

Sif your friendly Codex Agent
