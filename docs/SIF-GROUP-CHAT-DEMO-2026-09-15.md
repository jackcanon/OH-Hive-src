# Human-driven group chat demo

Sif your friendly Codex Agent — 2026-09-15

Implements Loki's `LOKI-TRACK-A-DEMO-TONIGHT-HANDOFF-2026-09-15.md` S-1 through S-5. A human can create a named Team or Project room, choose agents, and request replies with `@name` or `@everyone`. Agents reply with their own author identity. Their replies still have an empty recipient list: no autonomous agent-to-agent exchange, causation migration, held deliveries, or 30-turn gate was added.

## Native demo

1. In Bots, register three local agents on this Mac and give them distinct names in Agent details (for example `Planner`, `Builder`, `Reviewer`). All three currently use this Mac's selected local model; this does not imply three separate models or computers.
2. Choose **New room**, name it, and select the three agents. The optional coordinator is metadata only, not a new orchestration policy.
3. Post `@Planner @Builder suggest one next step each.` Two agents should reply, with their names above their messages; Reviewer should remain quiet. A post without mentions should produce no replies.
4. For the project demo, use **Load my Hive projects** and choose a real visible community project. This saves a project reference in a private room; it does not share the transcript with the community, load the project's source files, or execute project tasks. Standalone private accounts can create Team rooms without calling the community service. A private-project catalog picker remains separate work.
5. Leave the room open after replies. No additional deliveries should appear from those replies. Reopen the app/room to check saved title, project reference and transcript with the real account.

Type single-token agent names: `[A-Za-z0-9_-]+`, matched without case sensitivity. Agents with spaced names can be renamed or included through `@everyone`. Unknown or ambiguous names appear in a “Not notified” note; they do not reject the post. Code fences, inline backticks and email addresses do not count as mentions. Self-mentions are excluded for agent authors. The resolver is a shared core function; storage remains the membership authority.

## Implementation

- Existing `conversations_create(agent_id)` remains the DM wrapper, preserving DM reuse. New `rooms_create` checks all active owned members, the 16-agent limit, project/kind consistency and coordinator membership before writing. Core membership checks remain authoritative on send.
- `room_agents` reads the actual conversation roster. Its remote endpoint derives the owner from the authenticated node binding and scopes the room before returning profiles. Mention resolution uses this roster, never every agent in the account.
- `BotsModel` has room selection alongside DMs, room-specific drafts/retry IDs, and saved resolved recipients on retries. Multi-author rows resolve names by author ID. No reader reparses messages to create work.
- Web desktop `TeamChat.tsx` adds the same room/project flow, calls the shared resolver via Tauri, retains failed drafts and request IDs, suppresses stale selection responses, and polls without overlapping requests. Web has typed mentions and no coordinator picker; native has the optional coordinator picker. Autocomplete is deferred on both surfaces.
- Tauri still depends on its existing community identity path. It does not implement native independent private enrollment or remote-primary selection. When `private-primary.json` exists, its local store refuses to open, preventing a divergent local history.

## Small schema addition

Room naming was absent from the existing conversation contract. Schema **10** adds nullable `conversations.title`; `Conversation` and `NewConversation` use a serde default for older wire data. No delivery-schema change was made. Migration tests preserve version-9 conversations and test title/project persistence across a real database reopen. CLI/Tauri constructors were updated for the additive field. Reserve schema 11 or later for subsequent delivery migrations. Use matching rebuilt binaries; old schema-9 binaries refuse to open a schema-10 database.

## Verification

Core suite: **160 passed, 1 ignored**. Native bridge Bots suite: **7 passed**. Swift BotsModel suite: **6 passed**. Rust FFI release build, Swift release build, web production build, Tauri/CLI checks, and core local-hub-only check passed. Diff whitespace checks passed. Tests cover mention edge cases, room ownership and membership, quiet posts, deduplicated retries, exact two-agent fan-out, no rows created by replies containing `@everyone`, and persisted project/title metadata. The executor test uses a controlled reply implementation; it is not a live local-model or visual UI acceptance test.

The updated `apps/desktop-swift/Hive.app` was assembled and ad-hoc signed for local testing. As with the existing packaging script, its FFI library points into this checkout; this is not a portable distribution build.

No production migrations, live project creation, external messages, private fleet enrollment, physical fleet operations, or remote inference were performed. Changes are left uncommitted for Loki's consolidation.

## Remaining work / review notes for Loki

- Live single-Mac demo with the user's selected model and real project is still required. Only local agents on the executing Mac reply; BYOK profiles and other-host agents may be listed but their runtime support is unchanged.
- Creation follows the handoff's create-then-join sequence. Member validation occurs first, but remote network failure between joins can leave a partially assembled room. An atomic room-creation transport operation with a request ID is a useful follow-up before relying on remote room creation.
- Keep agent replies recipient-free until Loki's budget/hold/release design is implemented and tested. No coordinator or loop-budget semantics were decided here.
- Host-authorized remote delivery, primary replication/transfer and private discovery remain their own fleet work items.
