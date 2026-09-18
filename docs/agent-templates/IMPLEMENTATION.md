# Agent tools implementation status

18 September 2026.

Implemented:

- Owner-saved, versioned agent policies; legacy agents default to no library grants regardless of capability_policy_ref text (migration 0023).
- Typed library search/read, authenticated local and remote dispatch, current document revisions, library/node grants and assigned-host checks. No shell or writes.
- Mandatory chat message, conversation, lease generation and conversation revision supplied by the executor, not the model. Every call checks running/unexpired delivery, membership/read access and policy version in the same transaction as the read. Cancelled, completed, expired and superseded attempts cannot read (migration 0024).
- Eight successful calls per delivery generation, authority-side receipts linked to that attempt. No source content or search terms in receipts. Search max 20 hits, read max 32 KiB, model result max 64 KiB, context max 256 KiB. Runner retains its 120-second timeout and shared execution slot.
- Local-model tool loop uses native structured function calls and feeds results back as tool messages. Tools support explicitly false fails clearly; unknown metadata is allowed for compatible servers. Opaque references and prompt instructions grant nothing.
- Desktop FFI wires authenticated local/remote Private Fleet sessions into the runner. Right-side agent panel offers Assistant and Library Researcher template drafts, editable bio/instructions and existing/custom avatar preservation, selected libraries, host-sharing guidance and explicit Save tool access. Bio/instructions remain under Save profile; applying a template alone does not change saved access. Other roles are not advertised as operational.

Verification: policy/remote persistence and fencing tests; mock model reads real scoped document content and is denied content after delivery cancellation; existing local runner and migration regressions; FFI check and native build recorded in continuity log. No real model inference is necessary for these tests.

Rollout: update primary and desktop workers together before using tool templates. Old RPC servers do not provide the policy/turn APIs. Library sources must already be indexed and shared with the assigned worker in Vault. This does not create or broaden node-level grants. The headless CLI runner is not wired to this tool loop yet; use updated Loki's Den desktop workers for the first library templates. Subscription/API agents remain tool-free. Broader Researcher web tools and Librarian/Developer/Reviewer/Integrator/Coordinator execution policies remain follow-ups.

No installed app or live database is changed by a source build. A coordinated install and real fleet library-use check remain separate from mock verification.
