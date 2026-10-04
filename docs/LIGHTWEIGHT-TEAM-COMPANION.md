# Lightweight team companion — product and implementation contract

Date: 2026-10-04. Owner: Jack. Author: Sif your friendly Codex Agent.
Working interface label: “Loki’s team”; final product name not selected.

## Goal and first-run experience

A human, ChatGPT, Claude, Hermes and local agents share projects, ask questions, exchange
work and produce reviewed results through one simple interface. The lightweight app works
alongside Loki’s Den and eventually without its full desktop interface. The same durable
project authority must serve both products.

Target setup: sign in once, create a project, connect available accounts or discover local
workers, choose an optional team, and describe the goal. Show which participants can reply
now, which require an open assistant session, which are unavailable, and which need access.
Never display “connected” solely because a profile exists. Avoid addresses, pairing codes,
model identifiers and credentials in normal setup; advanced options stay available.

## Interface

Three main surfaces: Projects, Team and Activity. A project presents a conversation,
current brief, work queue, attached evidence and final results. A user can ask the whole team
or a named participant; “Work on this” creates a task rather than relying on unbounded chat.
Pause all work and per-task stop are always visible. Team templates suggest distinct roles
and remain editable; they do not enforce a particular model or roster.

Use accessible names and avatars, but preserve stable participant identity independent of
names and machines. Human messages must be human-authored; never route the human composer
through an agent credential. Profile presence, transport health and active execution are
separate states. Mark queued, working, needs input, ready for review, approved and failed
using actual persisted events. A visible message is not proof of work completion.

## Shared service and adapter boundaries

Reuse the Den's project rooms, message history, tasks, evidence and permissions. The new
companion is a small client, not a second task database. Package a headless background
service for people who only want the companion. A hosted gateway accepts authenticated
outbound connections from private services, with resumable event cursors and bounded queues.
Do not expose the internal remote procedure endpoint or fleet credentials.

Use the Model Context Protocol (MCP) connector for assistants interacting from their own
sessions. Use supported worker adapters for automatic execution: local model services,
Hermes and supported cloud execution clients. Adapter capability records must distinguish
context reading, posting, accepting tasks, available tools, cancellation and result receipts.
Provider authentication and billing remain separate from app sign-in. Never borrow consumer
browser session cookies or imply that a paid chat account universally grants unattended use.

External assistants only receive explicitly shared project context. Separate credentials
for each participant and client; owner chooses read, contribute and task execution permissions.
Shared files remain at their source; passing a file reference does not grant filesystem access.
The first connector provides read/contribute, not execution authority.

## Work and quality contract

1. Human supplies goal, constraints and acceptance criteria. Planner records a bounded plan.
2. Assign each task to one accountable worker; parallel independent tasks are allowed.
3. Worker acquires an expiring claim with heartbeat. Durable request identities prevent
   duplicate dispatch after lost responses; uncertain submissions require reconciliation.
4. Worker saves source-linked research or code changes with provenance, actual model,
   input revisions and verification results. A partial response remains partial.
5. A different participant reviews the saved evidence. Bounded correction rounds return
   actionable findings; timeout or exhausted budget stops visibly.
6. Human approves consequential publication or deployment. Project history records the
   decision and the exact evidence reviewed; stale results cannot approve later changes.

Ordinary chatter does not broadcast to every agent automatically. Explicit task assignment
or mentions determine delivery, with per-task turn, time and cost limits and cycle detection.
Avoid agents taking turns congratulating each other. Idle workers can be resumed by the
scheduler; an idle consumer chat cannot be treated as a reliable background worker.

## Implementation sequence and acceptance gates

A. Shared-context companion: project list and actual room history; no parallel store or
credential in browser. Implemented development source in `apps/companion`, six companion loopback
tests pass; human-authored updates, Enter-to-send and named current participants added. Actual Den and visual checks remain pending.

B. Connection and identities: packaged service startup, owner sign-in, named human and
external participants, client consent/revocation, scoped gateway, account capability checks.
A fresh user must join one project without entering an address or secret.

C. Conversation and bounded tasks: human composer, selective agent delivery, task assignment,
worker availability, durable claims, stop/reconnect and artifact receipts. Restart during an
in-flight task must not run it twice. A denied permission must explain how to resolve it.

D. Independent review and dogfooding: real research task through one cloud and one local
participant with source evidence; a real isolated code change through coder and independent
checker. Show all progress in the companion and Den with matching histories. Never pass
fixture or manually fabricated results as an operational agent run.

E. Distribution: signed installer or hosted client with the background service, supported
account walkthroughs, automatic updates, resource limits and recovery. Dependency security
checks must pass before release; the inherited Wasmtime 48.0.3 advisories remain unresolved
at this writing.

## Current limits and sources

OpenAI currently documents personal Pro custom connectors with read/fetch permissions and
full custom connector write support on organizational plans. Plus availability and published
app routes must be verified independently. OpenAI also documents a Secure MCP Tunnel for
private servers; evaluate it as an OpenAI-specific option, not a universal Claude/Hermes
transport. Connector support is not automatic execution support.

Source: https://help.openai.com/en/articles/12584461-developer-mode-and-mcp-apps-in-chatgpt
Verified 2026-10-04. Claude provider connector reference:
https://platform.claude.com/docs/en/agents-and-tools/mcp-connector . Runtime-specific Hermes
integration must be verified against the installed version before claiming support.

No accounts were connected, credentials issued, agents started or services published for
this development slice. Public gateway hosting/budget and live account setup remain open.
