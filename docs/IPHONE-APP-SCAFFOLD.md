# iPhone App Scaffold (ADR-021) — What's Here and What You Need to Do in Xcode

Written 2026-09-09. Everything below is source code I could write without a Mac/Xcode in front of
me — I could not compile or run any of it tonight (see the caveat at the bottom). Treat this as a
strong first draft to open in Xcode and fix forward, not a finished, verified app.

## What's in `apps/mobile-swift/`

A SwiftPM package (`Package.swift`, targets iOS 27) with a `Sources/OHHiveMobile/` tree:

- `SupabaseConfig.swift` — the shared `SupabaseClient`, pointed at the real project
  (`pxfbnuxcnerulbvbmowz`) with the real publishable key. Nothing to fill in here.
- `AuthManager.swift` — Sign in with Apple (fully wired: nonce generation, `ASAuthorizationAppleIDProvider`,
  exchanged via `signInWithIdToken`) and a **stubbed** Google Sign-In (see below — needs one more
  package added in Xcode before it does anything).
- `OHHiveMobileApp.swift` / `RootView.swift` — the app entry point; switches between `SignInView`
  and `MainTabView` based on session state.
- `SignInView.swift`, `MainTabView.swift` — sign-in screen (Apple + Google buttons) and a four-tab
  shell: Kanban, Projects, Wallet, Nodes.
- `KanbanStore.swift` / `KanbanView.swift` — local idea board (same shape as the Mac app's, ported)
  plus a live read of your real cloud projects via `hive_projects_overview`.
- `ProjectsListView.swift` / `ProjectDetailView.swift` / `NewProjectView.swift` — browse projects,
  create a new one by chatting with the interviewer (via the `interview` Edge Function), view/fund
  a project, and read/post its forum.
- `WalletView.swift` — $honey balance + recent activity via `hive_my_wallet`.
- `NodesView.swift` — your nodes and a real remote-checkout button, backed by two brand-new RPCs
  I added tonight (`hive_member_nodes`, `hive_member_node_checkout`) — these are deployed and live
  already, unlike everything else here.
- `Models.swift` — the `Decodable` structs the views above use.

## What you need to do in Xcode this morning

SwiftPM alone can't produce an installable/runnable iOS app — there's no way around opening Xcode
for this part:

1. **File → New → Project → iOS → App.** Name it `OHHiveMobile`, interface: SwiftUI, language:
   Swift. Put it somewhere convenient (doesn't need to be inside this repo, though it can be —
   e.g. `apps/mobile-swift/OHHiveMobile.xcodeproj` alongside the package).
2. **Delete the generated `ContentView.swift` and the generated `OHHiveMobileApp.swift`** — you'll
   use the ones from this scaffold instead.
3. **Drag every file from `apps/mobile-swift/Sources/OHHiveMobile/` into the new project** (check
   "Copy items if needed" and add to the app target).
4. **Add package dependencies** (File → Add Package Dependencies):
   - `https://github.com/supabase/supabase-swift` (this scaffold already assumes it, `from: 2.20.0`)
   - `https://github.com/google/GoogleSignIn-iOS` (`from: 7.1.0`) — needed to make the stubbed
     Google button in `AuthManager.swift` actually work; the commented-out code there shows the
     shape once it's added.
5. **Enable "Sign in with Apple" capability** — target → Signing & Capabilities → + Capability →
   Sign in with Apple. Needs your Apple Developer account attached to the project.
6. **Google OAuth client ID**: create one for iOS in whatever Google Cloud project backs the web
   app's own Google sign-in (Google Cloud Console → Credentials → Create Credentials → OAuth
   client ID → iOS, bundle ID matching this app's). Add the resulting reversed-client-ID URL
   scheme to Info.plist (Google's SDK docs show the exact key) and initialize `GIDSignIn` with the
   client ID at app launch.
7. **Confirm Supabase accepts native ID-token sign-in** — Supabase project → Authentication →
   Providers → Apple/Google should already be on (the web app uses them), but native
   `signInWithIdToken` sometimes needs the iOS bundle ID / Google iOS client ID registered
   specifically, not just the web OAuth client. Check the Supabase dashboard's provider settings
   if sign-in fails with a client-mismatch error.
8. **Build and run on a simulator.** Sign in with Apple works in Simulator; Google Sign-In does
   too once step 4/6 are done.

## Known rough edges to expect and fix

- **`WalletView.swift` and `ProjectDetailView.swift`'s field names are best-effort guesses.** I
  confirmed the RPC names by reading the web app's own calls (`hive_my_wallet`,
  `hive_project_board`, `hive_project_comments_list`, etc.) but did not read every one of their
  SQL definitions to confirm exact JSON key names — `Models.swift`'s `WalletInfo`/`WalletEntry`/
  `ForumComment` structs may need their `CodingKeys` adjusted if decoding fails on first run. The
  error messages in each view are written to surface exactly this ("Couldn't load wallet...") so
  it should be quick to spot and fix.
- **`NewProjectView.swift` only implements the provider-backed interview path**, not the web app's
  local-text-pool fallback (used when no cloud provider/BYO key is available and a compute node is
  online). That dual routing is real logic in `apps/web/app/new/page.tsx` worth porting later —
  skipped here for time, not by accident.
- **`supabase.rpc(...params:...)` call shapes are written from the supabase-swift API as I
  understand it, not verified against the actual installed package version** — if the compiler
  complains about the `params:` argument type or the `Encodable & Sendable` dictionary in
  `ProjectDetailView.fund()`, check `supabase-swift`'s current `rpc` signature; it may want a
  concrete `Encodable` struct instead of a heterogeneous dictionary literal.
- **Google Sign-In is a stub.** `AuthManager.signInWithGoogle()` currently just sets an error
  message. The real implementation is sketched in a comment in that file.

## The bigger caveat: none of this has been compiled

Same situation as tonight's other Swift/Rust work (see `docs/TELEGRAM-INTEGRATION-PLAN.md`'s
build-verification note): my sandbox's shell tool failed and never recovered, so nothing here has
seen a compiler. This was written carefully, cross-referencing the web app's real RPC calls and
`supabase-swift`'s documented API shape rather than guessing blind, but "carefully written" is not
"verified." Expect a handful of real compile errors on first open — none of them should be
architectural (the RPC names and app structure are grounded in the real backend), just the kind of
thing a first `⌘B` in Xcode always turns up.
