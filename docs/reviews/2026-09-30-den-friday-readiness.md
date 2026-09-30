# Loki’s Den: project autonomy and Friday readiness

Reviewed 2026-09-30 by Sif your friendly Codex Agent. Baseline: `06a418d`. Friday target: 2026-10-02. This is an implementation assessment, not a claim that live autonomous projects have passed.

## Decision

Finish two complete, bounded local workflows before expanding the team: a coding project with host-verified checks, and a research project with citations, independent review and a saved Library artifact. Use the existing hub, task engine and workspace machinery. Templates remain suggestions; selecting a role must never silently grant tools or claim unsupported runtime capabilities.

The main blocker is connective infrastructure, not a shortage of templates or models. Bots conversations, agent handoffs and coding cards currently operate through different paths. An agent can be called a coder without being able to submit coding work from its conversation.

## What exists and what remains

| Capability | Evidence in current source | Readiness |
|---|---|---|
| Named local agents, biography, instructions and tool grants | Bots profiles and agent inspector | Present; host/model/tool readiness still needs a project preflight |
| Scoped Library reading, web fetch and web posting | `bots/library_tools.rs` | Present; allowed destinations and collections must be configured |
| Explicit agent handoffs and target resolution | `local_hub/agent_tools.rs:412`, `local_hub/bots.rs:1275` | Present; tool-loop selection defect fixed on this review branch |
| Coding task staging, checkout preparation and host checks | `local_hub/private_code_tasks.rs:21`, coding worker and workspace modules | Present, separate from Bots |
| Coordinator child creation, waiting and restart context | `coder.rs:1514`, `coder/coordinator.rs:26` | Present and tested with mock workers; not proven in a live project during this review |
| Named Bots agent starting a coding project | Tool catalog and private submission shape | Missing: no coding-task tool, agent identity binding or coordinator option in the private request |
| Research report saved by a scoped agent | Bots tool catalog | Missing: no report writer in this tool loop; needs a host-owned artifact service |
| Unbounded mention-driven collaboration | `bots/executor.rs:141`, `ohhive-ffi/src/bots.rs:575` | Disabled by default; do not simply enable globally |
| Private cloud/external agents performing the same project work | Cloud runner and private Bots eligibility | Incomplete; provider credentials or subscription login alone are not a full agent runtime |

## Highest-priority findings

1. **Project execution is disconnected from agent delegation.** `bots_agent_handoff_create` writes `project_id: None`, no artifact references, no allowed tools and no parent run (`local_hub/agent_tools.rs:450–456`). Its wake message reaches a conversation, not a prepared coding checkout. Add a scoped project-run bridge to the existing private task engine rather than putting arbitrary command execution in ordinary chat.
2. **Delegation-only agents were denied their tools.** The local runner entered its tool loop only for Library reading or web fetching. An agent with only teammate or web-post permission therefore received no tools; an incoming handoff also could not activate a resolver-only loop. This branch considers all supported grants plus an incoming nonempty task reference, records that reference on new wake messages, and describes the actual available tools in the prompt. A reference selects a loop, never authority: the hub still validates live delivery, assigned host, policy revision and exact handoff target.
3. **Live execution readiness is not advertised.** Read-only Asgard inspection found zero rows in `private_coding_readiness`. Do not treat this as proof that model servers are offline, or the unrelated node check-in flag as Bots liveness. It does mean a fresh coding-worker capability has not been advertised through that registry.
4. **The currently configured local team cannot perform the proposed chain.** No active local coordinator has outgoing teammate grants. Tyr has no outgoing handoff grants. Hel has Library access and one posting destination, but no web-fetch hosts. Cloud agents have some handoff grants, but their current runner does not share the local tool loop. Role labels and persisted policy are insufficient evidence of operational capability.
5. **Recovery and result attribution still need work.** Handoff creation/wake and resolution/receipt use separate transactions. Interrupted publication can strand a requested or completed record. Scheduled result selection can take the first later reply rather than one tied to the exact delivery. Add durable publication reconciliation and exact run/delivery references before relying on unattended recurring work.
6. **Cancellation must fence publication.** A runner finishing after stop or reclaim must not publish an authoritative result. Keep live-generation checks at the result commit, not only at tool authorization. Cover a delayed mock response arriving after cancellation.

## Architecture decision alignment

The relevant Architecture Decision Records (ADRs) are 024 (coding), 025 (local hub), 028 (Library), 031 (external adapters), 032 (durable coordinator), 035 (Bots), 036 (workspaces), 037 (integrator) and 038 (concurrent agents).

Keep ADR-032’s existing child-card coordinator, ADR-036’s managed checkouts and ADR-038’s isolation rules. Connect named agents to those primitives through an owner-approved project run containing project, agent, host, model, tool scope, limits, parent and artifact references. Preserve source provenance and final host check receipts across every transition. A single project timeline should show who is working, what is waiting, what failed, and which result is verified.

Cloud agents and Hermes should implement the same capability-tested adapter contract later: execution, cancellation, restart, tool scope, artifact return and accounting. Do not advertise parity based only on successful sign-in or plain-text chat.

## Build order and acceptance gates

### Today: first complete local coding run

- Land the isolated handoff fix after review; deploy matching hub and worker builds. Existing historical handoff wake messages have no task reference: inspect the four requested live handoffs before deliberately retrying them; do not silently duplicate them.
- Advertise a fresh worker on Niflheim, confirm a downloaded tool-capable model, and configure one local coordinator with explicit teammate grants. Keep model inference off Midgaard unless Jack specifically authorizes it.
- Extend private project submission with an explicit coordinator option and named-agent binding, validated against owner, host, model and tool grants. Expose one “Run with my team” action in the project view. Reuse staging/preparation and bounded child creation.
- Start with a small existing test repository. Coordinator dispatches a scoped edit, waits, receives host checks, and returns a reviewable change. No automatic merge or push.
- Acceptance: run starts in Den; child targets the selected worker; checkout is isolated; required checks execute on the host; failing checks block completion; final report links the change and check evidence. Close/reopen the submitting app while the child runs and confirm no duplicate work.

### Next: complete local research run

- Use a researcher with explicit Library collections and a small allowlist of source hosts; choose a second agent for source checking.
- Add a host-owned report writer scoped to the selected project/collection. The model supplies report content and source references, not arbitrary filesystem destinations.
- Save the draft, send the checker its artifact reference, and persist the reviewed report with source URLs or document paths/revisions. Distinguish unsupported claims and unavailable sources.
- Acceptance: request starts in Den; researcher reads actual sources; checker receives the saved draft; final report is visible in Library/project view; every factual claim has evidence or explicit uncertainty; no file is saved outside the approved destination.

### Thursday: reliability and repeated dogfooding

- Exact reply/delivery association; durable handoff publication recovery; cancellation publication fence; finite turn/time/child limits.
- Run coding success, deliberate check failure, offline-worker recovery and stop during inference. Run research with an unavailable source and conflicting evidence. Repeat each happy path from a fresh run.
- Use Den to create a small real Den maintenance task and a project-local research brief. Record blockers as project tasks instead of manually patching runtime state to conceal them.

### Friday: demonstration gate

Both flows must succeed from the application with visible progress and saved results, without manual database edits, hidden terminal submission or repeated setup repair. Keep one known-good model and a small bounded team. If either gate fails, demonstrate only the flow that passed and state the remaining limitation. Full nine-agent autonomy, cloud parity, broad recurring scheduling and automatic integration are not prerequisites for these first two flows.

## Verification performed

Feature set: `local-hub,llama-cpp,bots,sandbox,desktop-provider,skills`, offline dependencies.

- Handoff tests: 11 passed, including new delegation-only, posting-only, incoming resolver-only and no-tools runner cases.
- Team tests: 3 passed, including failed-check propagation and unavailable-child targeting.
- Coordinator recovery test: 1 passed, including duplicate-spawn and unverified-success rejection.
- Local runner tests: 10 passed, including cancellation, timeout, wrong host and mock transport cases. These overlap the handoff set; totals must not be added as unique tests.
- Formatting and whitespace checks passed.
- Tests used temporary databases, checkouts and mock loopback model services. No local model inference or fleet mutation occurred.
- Read-only Asgard inspection: hub process present and listening on its private network address; database queries confirmed the configuration gaps above. An HTTP response alone is not proof of complete application health.

Not verified: installed app behavior after this change, real multi-agent project completion, current remote model inventory, full workspace suite, Swift build, cloud provider compatibility or external agent adapters. No deployment was performed. The shared checkout’s existing Swift branding changes were preserved.
