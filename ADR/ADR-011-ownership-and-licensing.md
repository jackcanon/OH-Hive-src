# ADR-011: Ownership and Licensing of Hive-Made Work

**Status:** Proposed · **Date:** 2026-09-04 · **Deciders:** Jack Blair (owner), Loki (architect) · **Source:** ADR-000 Q2, Q7, Q11, Q15; D8, D23, D37, D53–D55

## Context

Every card in the Hive is executed on a machine that belongs to someone other than the project owner, using models the contributor installed, for a reward in $honey (D23). Without an explicit rule, that arrangement invites the question of who owns a film cut, a music track or a codebase that fifty contributors' GPUs produced. Q15 settled it: the project owner owns it, contributors are paid and acquire no rights (D53).

At the same time, Q2 made the Hive radically transparent inside its walls: every member can inspect every project (D8), and nothing is exposed to the open internet or non-members. Transparency and ownership have to coexist, so the log draws the line at *inspect versus reuse* (D55). Seeing is a membership right; copying, forking and redistributing are governed by a license the owner chose.

That license is captured at the moment the project is born. The interviewer agent (D37) asks whether the project is open source or solely owner-owned and, for open source, which specific license (D54). Because this is stored on `hive.projects` from the first row, the web app can enforce fork visibility (ADR-009) and the node app can state the rules in the contributor Terms of Service (ADR-010) without any later back-fill.

## Decision

1. **Ownership defaults to the project owner** (D53). The `owner` role in `hive.project_roles` (D6) is the rights holder for all outputs of the project's cards, including intermediate artifacts and checkpoints. Admins and followers acquire no ownership through their role.
2. **Compute and storage contributors acquire no rights** (D53, D23, D50). Earning `earn_compute` or `earn_infra` ledger entries is the entire consideration. The node core writes outputs to Hive artifact storage and the local scratch directory only; the scratch directory is purged when a lease ends (ADR-006).
3. **License is set by the interviewer at creation** (D54). `hive.projects.license` is an enum `owner_only | open_source`, NOT NULL. When `open_source`, `hive.projects.license_spdx` holds an SPDX identifier (e.g. `MIT`, `Apache-2.0`, `CC-BY-4.0`) and is NOT NULL via a check constraint; when `owner_only` it must be NULL. The interviewer's structured output schema (Q11) includes both fields, and the interview cannot complete without them.
4. **Inspect is not reuse** (D55, D8). Any active member may view any project, card, conversation and artifact in the web app. Only `open_source` projects may be forked, downloaded for reuse, or incorporated into another project. `owner_only` artifacts are viewable in-Hive but not redistributable; the web app does not offer bulk download for them.
5. **Fork is gated on `open_source`** (D55). Fork copies the plan, cards and artifact pointers into a new project under the forker, records `hive.projects.forked_from` and the license of the source, and inherits the SPDX identifier (the forker may not relicense more restrictively than the source license permits). The fork control is not rendered for `owner_only` projects (ADR-009).
6. **License change is one-way** (Q15 implication). An owner may change `owner_only → open_source` at any time. `open_source → owner_only` is refused by a database trigger; the log notes this is in particular because others may already have forked. The trigger also refuses changing `license_spdx` once at least one fork exists.
7. **Terms of Service — required content** (Q15 implication; blocking for launch). The contributor ToS, accepted at node registration (ADR-010) and stored with version + timestamp, must state at minimum that the contributor:
   - provides compute and/or storage to the Hive and is compensated in $honey only;
   - claims no ownership, authorship or license in any output produced on their node;
   - may view any project as a member but agrees not to copy, redistribute or reuse `owner_only` material outside the terms the owner has granted;
   - understands that `open_source` material is governed by the SPDX license the owner selected;
   - understands $honey is a closed-loop credit with no fiat cash-out (D67).
   A member ToS accepted at invite acceptance (ADR-008) carries the same inspect-not-reuse clause for members who never register a node.
8. **No external exposure of any Hive data** (D8, D67). No project, card, artifact or ledger data is reachable without an authenticated active member session — regardless of license. `open_source` describes what members may do with the work, not whether the public can see it. Public project pages are out of v1 scope (D67) and would require a separate decision.
9. **Provenance is recorded, not asserted.** `hive.artifacts` rows carry `produced_by_card`, `produced_by_node`, and the lease id; the ledger carries the earning entry. This lets an owner demonstrate the chain of production if a dispute arises, without granting the node any rights.

### Schema sketch (`hive`)

```sql
create type hive.license_kind as enum ('owner_only', 'open_source');

alter table hive.projects
  add column license      hive.license_kind not null,
  add column license_spdx text,
  add column forked_from  uuid references hive.projects(id),
  add constraint license_spdx_required check (
    (license = 'open_source' and license_spdx is not null)
    or (license = 'owner_only' and license_spdx is null)
  );

-- trigger: refuse open_source -> owner_only; refuse spdx change once forks exist
-- node registration: hive.nodes.tos_version text not null, tos_accepted_at timestamptz not null
```

### Enforcement points

| Rule | Where enforced |
|---|---|
| Members can read all projects | RLS on `hive.*` via active `hive_members` (D35) |
| Fork only when `open_source` | RPC `hive.fork_project` checks license; UI hides control |
| One-way license change | BEFORE UPDATE trigger on `hive.projects` |
| No bulk download of `owner_only` | Web app gateway resolver refuses archive requests; single-artifact view allowed |
| Contributor accepted ToS | Coordinator refuses leases to nodes with `tos_version` older than current required |
| No external exposure | No anon policies on `hive.*`; no public Storage buckets; regional server gateway requires member token |

## Consequences

### Positive
- Clear, single answer to "who owns this" that matches how the community already thinks about commissioned work: the person who set the brief owns the result.
- License captured at birth means no retroactive licensing scramble and no unlicensed projects in the browser.
- Inspect-not-reuse preserves the "almost open source" culture Jack asked for without forcing owners to give away work.
- One-way license changes protect forkers from having the ground moved under them.

### Negative
- Technical enforcement of "do not redistribute" stops at the Hive boundary; a member can always screenshot or download a single artifact they can view. The ToS is the real control.
- Requiring an SPDX identifier during the interview adds a question non-technical members may not know how to answer; the interviewer must offer a short recommended list.
- A ToS is a legal document; engineering can write the requirements but not the final text.

### Risks & mitigations
- **ToS not ready at launch** blocks node registration entirely. Mitigation: treat ToS text as a launch-critical deliverable owned by Jack, tracked in Cmd Work; ship with a versioned placeholder only in staging.
- **Contributor disputes over outputs** (e.g. a contributor claims a track was made on their machine). Mitigation: provenance records (decision 9) plus the accepted ToS version.
- **Owner changes license after members have viewed but not forked.** Mitigation: viewing confers no rights either way; only forks are protected, and forks are recorded.
- **Third-party model licenses** (some image/video model weights restrict commercial use). Mitigation: the model catalog records each model's license; the interviewer warns when an `owner_only` commercial project would run on non-commercial weights. Open — see below.

## Open questions
- Which SPDX identifiers does the interviewer recommend by default? (Default: `MIT`, `Apache-2.0`, `CC-BY-4.0`, `CC-BY-SA-4.0`, `CC0-1.0`; free text for others.)
- Do model-weight licenses constrain the project license, and does the scheduler enforce that? (Default: warn in the interview and record on the card; no hard scheduler block in v1.)
- Can an `owner_only` owner grant per-member reuse permission without going `open_source`? (Default: no in v1; ownership transfer and per-member grants are a later ADR.)
- What happens to ownership when the owner's membership lapses or is revoked? (Default: project is frozen, owner retains rights, artifacts follow the D51 return path.)
- Does an admin who contributes creative direction (not compute) have any moral-rights claim under the ToS? (Default: no; admins accept the member ToS which assigns outputs to the owner. Legal review needed.)

## Related
- ADR-001-hub-and-source-of-record
- ADR-002-honey-economics
- ADR-006-agent-runtime-and-sandbox
- ADR-007-artifact-storage
- ADR-008-auth-and-membership
- ADR-009-web-app
- ADR-010-node-desktop-app
- ADR-012-scope-and-roadmap
