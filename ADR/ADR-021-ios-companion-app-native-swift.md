# ADR-021: iOS Companion App — Native Swift, Shipping This Week

**Status:** Proposed · **Date:** 2026-09-09 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** Jack's standing platform call ("we will always select native Swift whenever we have the option") plus a concrete this-week scope: Kanban view, project creation, agent chat, viewing/posting to each project's forum, checking servers/agents in and out, viewing $honey balance, and adding $honey to a project. Supersedes ADR-012 decision 15's v1.1 mobile plan (Expo/React Native) for iOS specifically.

## Context

ADR-012 (2026-09-04) scoped the mobile app as v1.1: "iOS + Android via Expo / React Native, sharing the React component package's tokens and logic layer... a wrapper plus native notifications, not a rebuild." That made sense at the time — no native app existed yet, and a cross-platform wrapper around the already-mobile-responsive web app was the fastest path to both phone platforms at once.

That premise changed the moment ADR-018 shipped a real native Swift macOS app. Jack's decision tonight makes the platform strategy explicit going forward: **native Swift whenever Apple's platform makes it an option, cross-platform tooling only where it doesn't** (Android still has no native-Swift option, so ADR-012's Android half is unaffected by this ADR — only the iOS half of decision 15 is superseded here). This is a standing principle, not a one-off choice for this app, and is recorded as such rather than only as a footnote to the mobile scope.

Investigating what a native iOS build actually costs turned up both good and sobering news:

**Good news — most of tonight's work carries over almost unchanged.** `hive-core`'s Cargo features are already split cleanly enough that an iOS build enables `hub` (+ `probe`) and never touches `wasmtime` (`sandbox`, iOS-hostile: its JIT codegen conflicts with App Store policy) or `tokio::process` (`tunnel`, iOS's sandbox forbids spawning subprocesses) at all — and a phone was never going to run those roles anyway, so nothing is lost by excluding them. Of the 13 Swift files in the macOS app, only 3 touch AppKit, and each just needs a trivial UIKit-analog swap (`NSOpenPanel` → `.fileImporter`, `NSAlert` → `.alert`, drop the menu-bar extra and Quit button — none of that applies to a phone anyway). `KanbanView`/`KanbanStore` (tonight's build), `ChatEngine`'s tool-calling shape, and `HiveStore`'s snapshot/pairing logic are plain SwiftUI/Foundation and port close to as-is.

**The real gap — the Mac app has never needed a member session, and this phone app can't avoid it.** Every must-have on Jack's list except the Kanban board's local-idea half requires acting *as a member* (`auth.uid()`-scoped RLS), not just as a paired node:
- **Project creation** goes through the same conversational interviewer the web app uses (`hive_interview_config`/`plan`/`poll`/`send`), which requires a member session to attribute the project to.
- **Forum viewing/posting** (`hive.project_comments_list`/`create`/`edit`/`delete`, from the project-forum migration) is member-authenticated.
- **$honey balance and funding a project** (`hive_fund_project` and whatever RPC the web wallet page reads) are member-scoped.
- **Checking servers and agents in/out** is the one item that isn't just "add member auth" — today, a node only ever checks itself in/out, using its own locally-held node key, from the machine it's running on (`HubClient::check_in`/`check_out`, called by the Worker on that machine). There is no existing mechanism for a member to remotely flip a *different* machine's presence from their phone. That's genuinely new backend surface, not a client-side port.

The Mac app's whole design (ADR-004: a node key, never a Supabase user JWT) was a deliberate choice for a single-purpose desktop utility. A phone app that does what Jack's list asks is a different kind of client — a first-party member app — and needs real member authentication (Sign in with Apple or Supabase magic link) as its foundation. This is the honest pacing item for "this week," not the Swift/UIKit porting work, which is comparatively small.

## Decision

### 1. Standing platform principle (applies beyond this app)

Whenever Apple provides a first-party framework/platform for something Hive needs, the native Swift/SwiftUI implementation is the default choice over a cross-platform alternative, for every current and future Apple-platform surface (macOS, iOS, and anything else Apple ships one for). Cross-platform tooling (React Native, Expo, Electron, etc.) is reserved for platforms Apple doesn't cover at all (Android, Windows, Linux). This doesn't reopen ADR-010's Intel-Mac/Tauri-permanently decision — that carve-out exists because pre-Apple-Silicon/pre-macOS-27 machines genuinely don't have the option (Foundation Models and this ADR's target APIs need macOS 26+), which is exactly the qualifier "whenever we have the option" already accounts for.

### 2. Scope: this week, six must-haves

Superseding ADR-012 decision 15 for iOS: not v1.1, not a wrapper — a native Swift iOS app targeting this week, covering exactly Jack's list and nothing beyond it for v1 of the phone app:
1. Kanban view (reuse tonight's `KanbanStore`/`KanbanView` directly for the local-idea half; the cloud-projects half already works node-scoped, no member auth needed for read).
2. Project creation (drive the existing interview RPCs, same flow the web app's `/new` page uses).
3. Agent chat — member-authenticated directly against the interview/project-conversation backend, not routed through ADR-020's Telegram-style link-code workaround (that indirection exists specifically because a Telegram account isn't a Supabase member; a first-party iPhone app with its own session doesn't need it).
4. View and post to each project's forum (`hive.project_comments_*`).
5. Check servers and agents in/out remotely.
6. View $honey balance and add $honey to a project (`hive_fund_project` for the latter).

### 3. Member authentication is the foundation, built first — both providers the web app offers

Confirmed against `apps/web/components/RequireMember.tsx`: the web app already offers **"Continue with Google" and "Continue with Apple"** (`supabaseBrowser().auth.signInWithOAuth({ provider: "google" | "apple", ... })`) — no email/magic-link option exists to match instead. So the iOS app matches both, not just Apple:
- **Sign in with Apple** via `AuthenticationServices` (`ASAuthorizationAppleIDProvider`), exchanged for a Supabase session with `signInWithIdToken(provider: .apple, idToken:, nonce:)`.
- **Google Sign-In** via Google's iOS SDK (`GoogleSignIn-iOS`, added as an SPM package dependency in the new Xcode project — a new external dependency this app needs that the Mac app never did), exchanged the same way with `signInWithIdToken(provider: .google, idToken:, nonce:)`. Needs a Google OAuth client ID registered for iOS (separate from the web app's client ID) and the resulting URL scheme added to the app's `Info.plist` for the redirect back into the app.

Both are net-new Swift code — nothing in `hive-core`/`hive-ffi` handles member auth today (pairing a node and authenticating a member are two different credentials, per ADR-004) — and remain the pacing item for the week, not the UI work. Apple's own review guidelines require offering Sign in with Apple whenever another third-party social login (Google, here) is offered, so building both together from the start avoids a rework later, not just a nice-to-have parity gesture.

### 4. Remote check-in/out needs a small new RPC, scoped conservatively for v1

Rather than building a full remote-command queue (a node polling for commands to execute) in one week, v1 ships the conservative half: a member-scoped RPC that verifies the caller owns the node (`hive.nodes.member_id = auth.uid()`) and flips `hive.nodes.presence` directly. **Checkout works cleanly this way** — the existing lease reaper already handles a node that stops heartbeating gracefully, so remotely marking a node `checked_out`/`draining` doesn't require that node's own process to do anything special. **Remote check-*in* is the part to be honest about**: flipping the database row to `checked_in` doesn't make a dormant Mac Mini actually start its worker process and connect to Ollama — the RPC can update the intended state, but actually bringing a machine online remotely (waking a sleeping Mac, starting the app) is out of scope for this ADR and this week. Ship the honest version: remote checkout works fully; remote "check in" surfaces as "request this node comes online" and only completes once that node's own app is already running and polls for the request — good enough for "I'm heading out, let me pull my laptop out of the pool from my phone," not yet "wake my machine from across the world."

### 5. Packaging: real Xcode project + xcframework, not just `swift build`

`Package.swift`'s current `-L../../target/aarch64-apple-darwin/release -lhive_ffi` linker hack doesn't work for iOS or App Store distribution. This week's build needs an actual Xcode project (or an SwiftPM library target consumed by one) plus a genuine `.xcframework` built from `hive-ffi` for `aarch64-apple-ios` and the simulator target via UniFFI's iOS bindgen path. One-time toolchain work, not ongoing cost once set up.

## Consequences

### Positive
- Nearly all of tonight's Rust/Swift investment (feature-gated core, Kanban, chat shape, HiveStore patterns) carries forward instead of being duplicated in a second RN codebase, vindicating the native-first call.
- A real, explicit platform principle prevents this exact "should this be RN or Swift" question from being re-litigated feature by feature going forward.
- Conservative remote-checkout scoping (§4) ships something genuinely useful this week instead of over-promising full remote wake/control and slipping the timeline.

### Negative
- Member authentication is new surface with real security weight (session handling, token refresh, sign-out) that the Mac app's node-only model never had to build — this is real, non-trivial work landing in the same week as five other features.
- Splits the app's identity model in two (node key for pairing/monitoring, member session for everything else) — needs to be designed coherently, not bolted on feature by feature, or the app will end up with inconsistent "why do I have to sign in twice" moments.
- Remote check-*in* being partial (§4) needs to be communicated honestly in the app's own UI ("request" language, not a toggle that implies instant effect), or it'll read as broken rather than scoped.

### Risks & mitigations
- **"This week" slips because member auth takes longer than expected.** Mitigation: build auth first, standalone and testable (sign in, see your own member profile), before touching any of the six features — if it's going to be the pacing item, better to know on day one than day four.
- **Xcode/xcframework packaging turns out gnarlier than the research pass suggested.** Mitigation: stand up the packaging skeleton (empty app, one screen, linked xcframework) as literally the first deliverable, in parallel with auth — proves the hardest unknown early.
- **Remote check-in/out scope creep** (someone reasonably asks "why can't it wake my Mac"). Mitigation: §4's boundary is written down here precisely so "not this week" has a citation, not just a verbal deferral.

## iOS 27 AI integrations to consider (advisory — not scoped into this week)

Jack asked what iOS 27 AI features are worth considering for this app, separate from the six
must-haves above. None of these are scoped into this week's build; they're candidates for after,
listed here so the thinking isn't lost:

- **On-device chat parity with the Mac app** (lowest effort of this list): `ChatEngine.swift`'s
  real, working `FoundationModels`/`Tool` pattern (ADR-015/018, shipped tonight for macOS) needs no
  new design to port — same framework, same APIs, available on-device on eligible iPhones. Scope
  it as a *separate* assistant from this app's "agent chat" (interview/project conversation, which
  needs live Hive data and has to stay server-backed): an on-device one answers "am I signed in,"
  "what's cached from my last sync" instantly and offline, the same division of labor `ChatEngine`
  already draws for the Mac app between local device-status Q&A and anything needing real data.
- **App Intents (Siri/Shortcuts)** — "Hey Siri, check my node out," "what's my honey balance."
  App Intents are a write-once, cross-Apple-platform framework, so this isn't iOS-only work: ADR-018's
  amendment already scoped App Intents as a macOS phase-1 item, and the same intents (backed by
  the same RPCs this app already calls) would work on both platforms from one definition.
- **WidgetKit home-screen widget** — $honey balance, or once ADR-020's Longest Hop feature ships
  real data, a "Longest Hop today" widget — exactly the kind of glanceable, shareable stat that
  feature was built for, and WidgetKit is the natural home for it beyond just chat-bot messages.
- **Live Activities** — a lock-screen/Dynamic-Island view of an active card lease or a "your
  project just got funded" moment, driven by the same `hive.notification_events` stream ADR-020
  built tonight (push-to-start via APNs once that event fires) rather than a fourth notification
  channel invented from scratch.
- **Visual Intelligence** (real but speculative fit) — camera-based recognition could plausibly
  seed an image/video project idea ("point your camera at this, start a project about it") for the
  image/video modality categories ADR-012 D60 already commits to supporting — flagged as a "maybe,
  later" idea rather than anything concretely scoped, since it needs product thinking about whether
  that's actually a workflow members want, not just a technically-possible integration.
- **Writing Tools** (essentially free) — standard SwiftUI text fields on iOS 26+ get Apple's
  system-wide Rewrite/Proofread for free with no app-specific work, so the chat and project-creation
  text fields in this scaffold likely already benefit without anyone building anything for it —
  worth confirming once running on a real device, not a build item.

## Open questions
- Confirm a Google OAuth client ID for iOS exists (or can be created) in whatever Google Cloud project backs the web app's own "Continue with Google," and confirm Supabase's project auth settings accept native `signInWithIdToken` for both providers (a slightly different flow than the web app's redirect-based `signInWithOAuth`).
- Exact RPC name for a member's own $honey wallet balance (the web wallet page has one; not confirmed by name during this ADR's research pass — verify before building the balance view).
- Should the six must-haves ship as one TestFlight build at week's end, or incrementally (e.g., Kanban + chat first, forum + honey + remote check-in/out second)? Default proposed: incremental, so partial progress is visible and testable rather than one big-bang build.

## Related
- ADR-012-scope-and-roadmap (decision 15, amended by this ADR for iOS — Android stays on the originally-planned cross-platform path since it has no native-Swift option)
- ADR-010-node-desktop-app (the platform-carve-out language this ADR's standing principle explicitly does not reopen)
- ADR-018-native-macos-swift-shell (the macOS app and Rust FFI feature-gating this ADR's iOS build reuses almost entirely)
- ADR-020-multiplatform-bridge-notifications-and-chat (the link-code identity pattern this ADR's chat feature explicitly does *not* need, being a first-party client)
- ADR-004 (node keys vs. member sessions — the exact distinction this ADR's §3 has to bridge)
