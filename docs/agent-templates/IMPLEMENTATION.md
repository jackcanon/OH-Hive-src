# Agent tools implementation status

18 September 2026, first foundation slice.

Implemented in local_hub/agent_tools.rs and named migration 0023-bots-agent-tool-policies:

- Owner-bound, versioned policy persistence. Legacy agents have no library grants, regardless of capability_policy_ref text.
- Typed library search/read calls. Unknown tools and unexpected fields are rejected. No shell, filesystem writes or library mutation exposed.
- Authenticated local and remote policy get/set and tool-execute APIs.
- Dispatch checks active agent ownership, exact assigned local host, current policy revision, selected library and existing node/library grant within the same database transaction as the read.
- Read requires current document revision, excludes archived documents and truncates content at 32 KiB with an explicit flag. Search treats terms literally, returns at most 20 hits and excludes archived documents.
- Successful reads record an agent/node/tool/library/policy-revision receipt without copying document content or search queries into receipts.
- Templates are versioned identifiers only at this stage. Picking a template identifier does not grant any permission automatically.

Not yet wired: Bots model tool-call loop, delivery-bound invocation context, user-facing template catalog/chooser, FFI policy methods and Tools and access inspector. Local authenticated RPC clients can exercise the foundation. Bots chats remain tool-free in current installed builds. No app install or live schema migration is part of this slice.

Before exposing the dispatcher to model turns, bind calls to the active delivery/conversation, enforce per-turn budgets/cancellation, include tool results as untrusted data, and check grants freshly for every call. Do not substitute prompt instructions for host enforcement. Cloud adapters need explicit supported routing before granting access; the first dispatcher currently accepts only the assigned local runtime host.

Next slice: thread this dispatcher through the local Bots runner and delivery lifecycle, then expose the read-only Researcher template with actual library selection. Other templates remain unavailable until their corresponding execution policies exist.
