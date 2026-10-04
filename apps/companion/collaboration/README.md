# Addressed project inbox — development setup

This optional background watcher reads the Den's existing project log every five seconds.
Idle checks do not invoke a model. Ordinary text never dispatches a turn. This is development
source, with fixture acceptance only; not an installed or live-account-verified integration.

The first adapter uses a dedicated Codex app-server process and an explicitly selected existing
session. It does not control arbitrary open desktop chats. Provider sign-in is managed by Codex;
no new product account is required. Model use can consume that provider's allowance.

## Explicit local setup

Use Node.js 20 or later, an authenticated Codex executable on the command path, the primary
project service, and an owner-issued agent project grant with read/contribute access.
The grant must belong to the mapped recipient. Keep the token in
`DEN_PROJECT_CONNECTOR_TOKEN`; `DEN_PROJECT_ENDPOINT` defaults to the primary loopback endpoint.
The token is excluded from the spawned harness environment.

Create a private configuration file (owner read/write only). Use real identities from the shared
project, an existing dedicated session and a project directory. The single mapping is deliberate:
run one watcher per participant with that participant's own credential and state directory.

```json
{
  "enabled": true,
  "stateDirectory": "/absolute/private/inbox-directory",
  "mappings": [{
    "room": "00000000-0000-4000-8000-000000000001",
    "recipient": "00000000-0000-4000-8000-000000000002",
    "session": "your-existing-codex-thread",
    "cwd": "/absolute/project-directory",
    "authors": ["user:00000000-0000-4000-8000-000000000003"],
    "startAfter": 123,
    "wakeResponses": false,
    "maxTurns": 3,
    "timeoutMs": 120000
  }]
}
```

`startAfter` is the reviewed project sequence to begin after, preventing old work from starting
unexpectedly. It seeds only a new inbox; restarting does not reset its cursor. Authors are exact
source identities, not names asserted in message text. Add an agent author and opt into
`wakeResponses` only for a deliberately bounded team exchange. `maxTurns` is a persistent
session budget (1–10), not a per-poll allowance. No automatic reset is implemented.

Launch from the repository root:

```sh
node apps/companion/collaboration/watch.mjs /absolute/private/config.json
```

`--once` performs one check and closes the dedicated process. Use it only for idle checks or
fixture acceptance; closing the process may interrupt active work. Long-running mode is required
for live turn completion. The first adapter uses a restricted project read-only filesystem sandbox,
disables approval escalation, and declines unsupported interactive requests. Existing provider tool
permissions remain distinct from filesystem permissions; this is not a grant of new connector access.

## Addressed entries

Post through the existing scoped project connector with a stable posting request identifier.
The body is a JSON envelope; source authorship comes from the service, not this envelope:

```json
{
  "protocol": "den.collaboration.v1",
  "type": "request",
  "event_id": "00000000-0000-4000-8000-000000000004",
  "to": "00000000-0000-4000-8000-000000000002",
  "depth": 0,
  "text": "Read the project design and identify the three highest-priority gaps."
}
```

Replies use `type: response`, `reply_to`, the next depth and the original author as recipient.
Depth four stops further dispatch. Normal composer text does not yet produce this envelope;
a friendly recipient picker and one-click setup remain future work.

## Receipts and recovery

The private sidecar stores message references, cursor, turn identifier and publication receipts,
not conversation bodies or tokens. It uses atomic replacement, file/directory synchronization
and one exclusive watcher lock. Queued work waits while the selected session has accepted or
uncertain work. Accepted work is completed only after saved harness output is read and posted
back to the project. Response publication retries reuse one request identity and verify the
response digest. A missing/changed result never counts as completion.

Submission errors and crash-boundary `sending` states become `uncertain`, which blocks that
session. Do not delete receipts and resend. Inspect the mapped harness's actual thread/turn to
reconcile first. There is no reconciliation interface yet. A crash leaves `watcher.lock`:
verify the previous process is gone before removing that lock and reopening the same directory.
The watcher never steals a live lock. State-corruption and capacity failures stop cursor advancement.

Tests cover restart, uncertain submission, duplicate-safe publication, unauthorized authors,
busy-session queueing, idle checks, bounded feedback, malformed pages, exclusive locks,
changed responses, timeout interruption and a launched harness fixture. No real model was run.
Claude Code, Hermes and ChatGPT event adapters remain separate next slices.
