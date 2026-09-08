# ADR-014: Project Categories, Tailored Creation Flow, and Triangulated Card Verification

**Status:** Proposed · **Date:** 2026-09-08 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** this conversation (Community Chat project retrospective; external review of https://github.com/aden-hive/hive/blob/main/docs/architecture/README.md)

## Context

Jack's framing for everything that follows: whether or not OH Hive becomes a lasting Office Hours community project, he wants to walk away with a system that coordinates his own local and cloud agents through one environment. That reframes a design question that would otherwise be "what would the community want" into "what does Jack's own work actually need" -- and Jack's own work spans more than software.

Two things converged to trigger this ADR:

1. **The Community Chat retrospective.** The first real project run through OH Hive's interview -> cards -> node execution pipeline produced a plausible-looking but non-integrated result: four of six cards were truncated mid-file, the backend card never wrote the REST endpoints the frontend card then assumed existed, two cards disagreed on an env var name (`JWT_SECRET` vs `JWT_SECRET_KEY`), and the plan itself picked a heavier architecture (a standalone FastAPI + Postgres + Docker microservice) than the goal needed. None of this was caught before the cards reached "review" -- the only gate between a card and `status = done` is a human reading raw text in a browser and clicking Accept. Chat itself was also re-scoped to an async, threaded, per-project forum board (see `20260908010000_project_forum.sql`) rather than realtime chat, since Discord already serves that need for Office Hours.
2. **External research.** `aden-hive/hive` (a same-named but unrelated single-operator agent-reliability framework, not a competitor to OH Hive's marketplace model) documents a shipped pattern -- *triangulated verification*: fast deterministic rules, then a confidence-gated LLM judge, then a human, in that order -- plus a reflexion loop that injects judge feedback into the next attempt instead of silently accepting or silently failing. OH Hive has neither today.

Separately, Jack asked for a first pass at project *categories* -- so that "create a project" can offer a tailored experience (different interview questions, different card templates, different acceptance conventions) depending on whether the work is code, research, transcription, or something else. This ADR records both: the category taxonomy, and the verification mechanism that every category will eventually share.

## Decision

### 1. Categories are a UX/prompt-template layer, not a schema change

A project **category** is not a new column or a new node capability. It is a fixed set of interview questions and a card-template convention that the `interview` Edge Function selects between before it starts the plan-building conversation. The underlying primitives it composes are already in place: `Modality` (text/code/image/video/speech/music), `required_capabilities` (the existing per-card jsonb flag bag -- `exec_wasm`, `artifact_put`, `spawn_child`, `model_id`, `tools_level`, etc.), and `spawn_child_card` (parallel fan-out, already shipped for sub-delegation, D44). Category != modality: Research is almost entirely `text` modality plus an internet-access flag; Media Generation spans four modalities on its own. This keeps the taxonomy cheap to add to and means a category can be listed in the creation flow long before its execution story is fully built -- as long as it's honestly labeled with its maturity.

### 2. The nine categories

| # | Category | Maturity | What's tailored |
|---|---|---|---|
| 1 | **Software / code development** | Partial -- see S3 | Stack, target platform(s), what "done" means (tests, not prose) |
| 2 | **Research & analysis** | Partial -- see S4 | Scope, depth, source constraints, output format |
| 3 | Content / creative writing | Placeholder | Voice/tone/length as acceptance criteria instead of tests |
| 4 | Media generation (image/video/speech/music) | Placeholder -- blocked on M8 | Style/prompt-driven cards across the four non-text modalities |
| 5 | Transcription & captioning | Placeholder -- blocked on M8 | Source audio/video, speaker labels, timestamp fidelity |
| 6 | Translation / localization | Placeholder | Source/target language pairs, terminology glossaries, fluency as the acceptance bar |
| 7 | Data processing / ETL | Placeholder | Cleaning/structuring/labeling a dataset the member already has (distinct from Research, which finds new information) |
| 8 | Review / QA | Placeholder | "Critique this, don't build it" -- code, design, or document review; naturally reuses the fan-out pattern in S5 |
| 9 | Operations / personal automation | Placeholder | Recurring or one-off multi-step agent work that isn't cleanly software or research -- the home for Jack's own "coordinate my agents" use case as a first-class thing, not a side effect of the other eight |

Categories 1 and 2 are specified in full below because they're buildable against what exists today (or what the cross-platform build/test discussion already scoped) and because Jack named both as strength-in-numbers fits. Categories 3-9 stay as placeholder rows -- picking one in the creation flow should say plainly that the category has no tailored backend yet, rather than silently falling through to the generic software template.

### 3. Category spec: Software / code development

**Interview questions** (in addition to the existing goal/title/license flow): target language(s)/framework, target platform(s) (this is where "Windows, Linux, macOS" gets asked explicitly -- see the cross-platform build/test discussion earlier this session), what a finished deliverable looks like (a runnable app? a library? a script?), and whether tests are expected to exist or be written as part of the work.

**Card template conventions:**
- Every code-producing card gets `required_capabilities.artifact_put = true` and is expected to write real files via the sandbox's scratch directory and `artifact_put`, not inline `### filename` text blocks in `card_outputs.content`. (The zip-export stopgap shipped tonight parses the `###` convention because that's what exists *today*; it is explicitly not the target state -- see ADR-006 and the multi-OS build/test discussion for why the sandbox can't run a real toolchain yet, and what a second, higher-trust execution tier would need.)
- A project-level card (or the plan itself) owns a single git repo per project; cards commit into it rather than each producing a disconnected fragment. This is the direct fix for the Community Chat integration-mismatch problem (Context item 1).
- Cross-platform test cards use `spawn_child_card` to fan out one child per target OS once `required_capabilities.os` exists as a node-matched field (not yet -- tracked against the cross-platform roadmap, not this ADR).

**Acceptance criteria convention:** structured, not prose -- a required list of files/paths, a pass/fail test command where applicable, hard constraints (e.g. "no `eval()`", "no plaintext secrets logged") in addition to the free-text `acceptance` field already on `hive.cards`.

**Where triangulated verification plugs in** (S6): rules first -- required file/path checks, banned-pattern checks, "does this reference an endpoint/env var no other card defined" cross-card consistency checks (this specific rule would have caught the `JWT_SECRET` mismatch). LLM judge second, scored against the structured acceptance criteria. Human (Jack, or the project admin) only sees cards the judge is not confident about.

### 4. Category spec: Research & analysis

**Interview questions:** the research question or scope, how deep (a quick summary vs. an exhaustive literature review), which sources are admissible (does it need `requires_internet`? Is search-only sufficient or does it need to fetch/read full pages?), and the expected output shape (a report, a comparison table, a recommendation).

**Card template conventions:**
- The plan's top-level research card sets `required_capabilities.spawn_child = true` and fans out one child card per sub-topic or per source cluster via `spawn_child_card` -- this is the "strength in numbers" mechanism Jack specifically flagged. D44 built one-child delegation; true N-way fan-out is not yet built -- see the correction in S5.
- Each child card carries `requires_internet: true` where it needs to fetch anything live, and is individually leased/priced like any other card.
- A synthesis card (parent, `wait: true` on its children per D44's pause/resume mechanic) merges the children's findings once all report back.
- **Pointer/spillover pattern** (borrowed from the aden-hive review, S6): a research card's tool results (a large web page, a long search result set) get written to the card's scratch directory rather than stuffed whole into the model's context, with a compact pointer + preview left in-line and a `load_data`-style tool to retrieve the rest on demand. OH Hive's sandbox already gives every card an isolated scratch directory (ADR-006); this pattern is new plumbing on top of an existing primitive, not a new trust boundary.

**Acceptance criteria convention:** source diversity (a minimum number of independent sources), citations required per claim, and a hard constraint against presenting a single source's claim as consensus.

**Where triangulated verification plugs in:** rules first -- are citations present, is the source count above the minimum, are there any dead/unreachable source links. LLM judge second -- does the synthesis actually answer the original question, is it fair to disagreeing sources. Human only on low confidence or a flagged factual dispute.

### 5. Shared fan-out mechanism -- correction: 1:1 today, N-way is new work, not reuse

**Correction (2026-09-08, later the same day):** the original text of this section claimed `spawn_child_card`'s `wait: true` mechanic "was built for sub-delegation (D44) and is being deliberately reused" for N-way fan-out. That overstated what D44 actually shipped. Checked against the real code before relying on it further:

- `SpawnChildSpec` (`tools.rs`) is a single struct, not a list -- a card's `required_capabilities.spawn_child` can declare exactly one child per tool pass, not several.
- The pre-Draft tool step runs **once, ever, per card's life** (`worker.rs` module doc: "a card can't yet ask for a *second* tool call mid-loop"). On resume from `WaitingOnChild`, the card goes straight to Draft -- it never re-enters the tool step to spawn a second child. So one card can pause on at most one child, one time.
- The parent-side state that tracks what a card is waiting on (`LoopState.pending_child_key`, `ToolPhaseResult.wait_on_child`, `HubClient::wait_on_child`) all hold a single child reference, not a set or count. There is no `blocked_on_child_ids` column -- only the generic `parent_card_id` back-reference.
- No test anywhere (Rust or SQL) exercises more than one simultaneous child under one waiting parent.

The one piece that's already right: the DB-side resume trigger, `hive.cascade_child_status()` (`20260907204830_hive_sub_delegation_pause_resume.sql`), resumes the parent by counting *all* of its children not yet in `review`/`done` -- not by checking the one child id that triggered it. That query is already correct for N-way join and needs no changes.

**What true fan-out (S3's per-OS test cards, S4's per-source-cluster research cards, category 8's per-reviewer cards) actually requires:** `SpawnChildSpec` becomes a list so one tool call can create N children; the worker's tool step needs to loop over that list instead of assuming one; `pending_child_key`/`wait_on_child` need to hold a `Vec`/count instead of a single id. This is a real, scoped Rust-layer change -- not a reuse of something already built, and not something this ADR schedules.

### 6. Shared mechanism: Triangulated Card Verification (proposed, not yet built)

Adapted from `aden-hive`'s triangulated-verification pattern (their Phase 1, the only phase they mark "Implemented" -- their own Phases 2-4, confidence calibration / rule generation / signal weighting, are marked "designed, not yet implemented" / "planned" / "conceptual," so this ADR adopts only the proven layer, not their aspirational roadmap):

1. **Deterministic rules, checked first.** Cheap, fast, zero false positives when written correctly: required files present, banned patterns absent, cross-card consistency checks. Priority-ordered; a match returns a definitive verdict without ever calling a model.
2. **LLM judge, confidence-gated, checked second.** Scores the card's output against its structured acceptance criteria (S3, S4) and returns a verdict plus a confidence number. Below a threshold, it doesn't guess -- it escalates.
3. **Human, last.** Jack (or the project admin) only sees cards that rules didn't resolve and the judge wasn't confident about -- not every card in "review," which is what happens today.
4. **Reflexion on RETRY.** When the judge returns RETRY, its specific feedback is injected as the next turn's context before the card runs again -- the model sees its own prior attempt plus the concrete critique, rather than silently shipping a half-finished file (the direct fix for the Community Chat backend card cutting off mid-class with nothing catching it).

This is a cross-cutting change to `hive.card_accept` and the worker step loop, not owned by any one category -- it benefits Software and Research immediately and every future category as it comes online. It is recorded here because it came directly out of this review, but it is its own build (worker.rs, a new judge step, `hive.cards`/`card_outputs` schema additions for storing verdicts and confidence) and is **not** scoped or scheduled by this ADR.

### 7. What explicitly does not transfer from aden-hive

Their Queen Bee has direct read/write access to a shared credential store across all of one operator's own trusted agents. That model assumes single-operator trust throughout. OH Hive's sandbox (ADR-006) exists specifically because a card's executing node cannot be assumed trustworthy -- importing shared-credential access across nodes would undo the reason that sandbox exists. Nothing in this ADR proposes adopting that part of their architecture.

## Consequences

### Positive
- The creation flow can honestly offer nine categories today without overpromising: two are specified and buildable, seven are visibly placeholders.
- Both flagship categories reuse existing, already-shipped primitives (`spawn_child_card`, the sandbox's scratch directory, `required_capabilities`) rather than requiring new core infrastructure before either can start.
- Triangulated verification directly targets the specific failure mode already observed in production (Community Chat) rather than a hypothetical one.
- Jack's own "coordinate my agents" use case gets a first-class home (category 9) instead of being an implicit side effect of "Software."

### Negative
- Two categories fully specified out of nine means the creation flow will look incomplete for a while; the placeholder categories need honest "not yet available" messaging or they'll generate the same kind of disappointment the Community Chat project did.
- Triangulated verification is a real build (worker loop changes, a new judge call, new columns to store verdicts) that this ADR deliberately does not schedule -- it can be cited as "planned" indefinitely if it isn't given its own milestone.
- The research fan-out pattern (S4) multiplies Honey cost by the number of child cards; there's no guidance yet in this ADR on capping fan-out width against a project's fund balance.

### Risks & mitigations
- **Placeholder categories ship as second-class and never get built.** Mitigation: track each as its own future ADR/milestone rather than a line item here, same discipline as ADR-012's milestone table.
- **Verification rules become a maintenance burden as categories multiply.** Mitigation: keep rules category-scoped and reviewed alongside each category's spec, not as one global rule set.
- **Fan-out cost surprises a project owner.** Mitigation: cap `spawn_child_card` width per parent card, surfaced in the funding UI before a research or cross-platform-test card runs (open question below).

## Open questions
- Does Triangulated Card Verification get its own ADR and milestone number, or land as an amendment to ADR-006 (sandbox/agent runtime) once scoped? (Default: its own ADR when someone picks it up -- it touches scheduling and card state, not just the sandbox.)
- Who writes the per-category rule sets (S6.1) -- Jack, category "owners" as they're built, or Loki drafting for review? (Default: drafted alongside each category's spec, reviewed by Jack before activation, matching aden-hive's own human-approval-before-activation step for learned rules.)
- Should a project be allowed to mix categories (e.g., a Software project with a Research card embedded), or is category chosen once at creation and fixed? (Default: category informs the initial interview and card templates only -- nothing stops a plan from including a card of a different shape once work starts.)
- What caps `spawn_child_card` fan-out width against a project's Honey balance? (Open -- not addressed by D44 or this ADR.)

## Related
- ADR-003-node-core-and-backends (Modality, Capabilities/Requirements matching)
- ADR-005-scheduler-and-leases (capability matching, spawn_child_card's D44 origin)
- ADR-006-agent-runtime-and-sandbox (scratch directory, why the sandbox can't run real toolchains, trust boundary for S7)
- ADR-007-artifact-storage (artifact_put/get, relevant to S3's "real files, not prose")
- ADR-012-scope-and-roadmap (M8 media backends, the milestone categories 4-5 are blocked on)
