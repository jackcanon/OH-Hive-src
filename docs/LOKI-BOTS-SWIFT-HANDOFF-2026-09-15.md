# Bots chat UI -- handoff for the Swift app (Hive.app)

Written 2026-09-15 by Loki, after wiring the Tauri desktop app's own team-chat screen against
your FFI bridge (`c9e6165`). This is the matching request for `apps/desktop-swift`. I have no
Xcode/Swift toolchain in my sandbox, so this is scoped, not built -- same shape as the FFI
handoff you picked up earlier today.

## What already exists for you to build against

`HiveStore` (`Sources/OHHive/HiveStore.swift`) owns the one `HiveNode` instance for the app's
lifetime and republishes FFI calls as thin `async throws` wrapper methods (see
`chatgptAccount`, `sendByokChat`, etc. for the pattern). `bots_open()`'s `BotsSession` isn't
wrapped there yet. Suggested addition:

```swift
@Published var botsAgents: [BotsAgent] = []
private var botsSession: BotsSession?

func botsOpen() async throws -> BotsSession {
    if let s = botsSession { return s }
    let s = try await node.botsOpen()
    botsSession = s
    return s
}
```

Every other Bots call (`agentsList`, `agentsCreate`, `conversationsList`, `conversationsCreate`,
`conversationsJoin`, `messagesList`, `messageSend`) is already exported on `BotsSession` itself
(`crates/ohhive-ffi/src/bots.rs`) -- a view can call `try await store.botsOpen().agentsList()`
directly, or `HiveStore` can grow one-line wrappers the same way `sendByokChat` wraps its call.
Records already generated for Swift: `BotsAgent`, `BotsConversation`, `BotsMessage`, `BotsPage`,
`BotsSend`.

## What's missing: the actual screen

`ContentView.swift`'s sidebar (`NavigationSplitView`, around line 75-85) switches on a
`Destination` enum -- `.setup`, `.chat(id)`, `.hiveProjects`, `.privateFleetBoard`, etc. Add
`.bots` alongside them, and a sidebar row for it (the `List(selection:)` block around line 150).

New views, matching `ChatView.swift`'s existing shape (a `.task(id:)` for load-on-select, an
`onAppear`/poll loop for incoming messages, a scrollable message list + input field at the
bottom -- see `ChatView.swift` lines ~40-150 for the pattern, and `ChatEngine.swift`'s doc on
why polling from `.task(id:)` rather than a separate timer avoids the two-tasks-race it calls
out):

- **Agents list** (or reuse a section of an existing settings/projects view): list
  `botsAgents`, a "Register this Mac" button when empty (`agentsCreate(name:)`, defaulting to
  the Mac's display name same as the Tauri app does).
- **DM view**: given a `BotsAgent`, find-or-create its DM conversation
  (`conversationsList()` filtered to `kind == "agent_dm" && coordinator == agent.id`, else
  `conversationsCreate(agentId:)`), then poll `messagesList(conversationId:page:)` with
  `after:` set to the last seen `server_sequence`, and a `messageSend(draft:)` on submit with
  `expected_policy_revision` from the conversation, matching what `ChatEngine.swift`'s existing
  send flow already does for the coder chat.

## What actually replies

Neither this bridge nor a Swift view starts a model runner (your own FFI handoff already says
so). The Tauri app now owns its own background drain loop
(`apps/desktop/src-tauri/src/bots.rs`'s `spawn_drain_loop`, polling every 5s via
`DeliveryExecutor`) so opening *that* app is enough for its local agents to reply. `Hive.app`
doesn't have an equivalent yet -- worth the same treatment (a loop spawned once, e.g. from
wherever `HiveStore`'s `init` or `startWorking()` already kicks off background work), or the
screen will show sent messages with no reply unless a `hive bots work` CLI process or the Tauri
app happens to be running as well. Your call on where that loop best lives in this app's
existing task structure -- I don't have enough context on `HiveStore`'s full lifecycle to place
it confidently myself.

## Not done here

No design/layout decisions, no dark-mode/accessibility pass, no handling for the
`conversations_join`-needs-authorization case (closed in core today, `5e25f1d` -- a Swift view
should never need to call `join` at all for its own owned conversations, `conversationsCreate`
already grants membership).
