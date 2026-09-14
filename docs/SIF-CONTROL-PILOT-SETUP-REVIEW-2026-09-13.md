> **Completed:** Jack approved this setup; the pilot ran and was cleaned up. See
> [completed report](SIF-CONTROL-PLANE-PILOT-HANDOFF-2026-09-13.md). Earlier approval requests below are historical.

# Prepared Chicago pilot setup

Status: prepared, not applied. Automatic approval review rejected the setup pending explicit
approval of its persistent database changes and 50-Honey allocation.

Claude's latest continuity entry records Jack's approval to duplicate the completed Hermes
walkthrough project. Chicago was rechecked: SSH works, the original Hive service is active,
and the final pilot Linux build succeeded. The original three cards remain in review.

## Exact proposed changes

1. Apply only `supabase/migrations/20260914010000_control_plane_pilot.sql`, adding the isolated
   pilot allowlist, hub-token records, and restricted control functions. Record that migration.
2. Create project `6045a447-04ae-4a2d-bf7a-860c3d63257b`, named
   **Hermes 3 via Ollama – macOS Walkthrough for Office Hours [Control-plane pilot]**,
   with three fresh copies of the original cards. Preserve the original project and outputs.
3. Set the new cards' model requirement to `sif-control-pilot-qwen35-9b`, a temporary alias of
   the existing local `qwen3.5:9b` model. This keeps ordinary workers that do not advertise that
   alias from taking the pilot cards. No model download is needed.
4. Allocate **50 existing Honey credits** from the owner's Hive wallet into the pilot project's
   fund using the normal `hive.fund_project` function. This is an internal allocation, not a
   purchase or an external cloud-model charge. Actual completion usage is metered normally.
   Read-only preflight found 9,305.956447 Honey in the wallet.
5. Create two clearly labelled pilot worker identities, with separate temporary keys:
   `3e68ff7c-06ad-412d-80c2-14fe1113dfd7` and `b9748395-b6cf-4e33-9345-0fe8d4e9f88b`.
6. Create database login `hive_ctl_chicago_pilot`, connection limit 3, schema usage and execute
   access only to the two pilot gateway functions. Bind it to Chicago, the one pilot project,
   and these two worker IDs. Generated credentials are stored in private files, excluded from
   reports and Git; the server environment file will have mode 0600.
7. Start the separate pilot binary on Chicago loopback port 8791. Temporarily add only
   `/hive/ctl/1/` to the existing Chicago HTTPS tunnel, preserving its current configuration.
   The original Hive service continues to serve its existing routes and direct-RPC remains
   authoritative. No global endpoint selection or privilege revocation is included.
8. Run the bounded pilot and its failure-path checks, capture actual results and latency, then
   disable the pilot allowlist, revoke temporary worker credentials, and stop/remove the pilot
   route. Preserve the duplicate project and its outputs for review. A community cutover is
   a separate decision.

## Prepared execution and verification

The transaction and credentials are prepared under `/tmp/sif-control-pilot-private/` (private
permissions). `setup.sql` contains credentials and must not be pasted into reports or chat.
The rejected call did not execute. A subsequent read-only database check confirmed the pilot
project, database role, and pilot table do not exist; the original cards are unchanged.

Implementation verification remains the 66-test workspace pass, four focused Linux tests,
Linux binary build, and rollback-only SQL integration checks documented in
`SIF-CONTROL-PLANE-PILOT-HANDOFF-2026-09-13.md`.

Signed: Sif your friendly Codex Agent
