# ADR-036: private coding request staging and child repository inheritance

Added a trusted local-host staging API for private coding requests. This is a backend dependency for authenticated preparation and job submission, not an exposed Run button or a running private worker.

## Submission contract

`LocalHubStore::stage_private_code_task` accepts a typed request: stable request UUID, project UUID, enrolled target node UUID, title, instructions, optional model, bounded turn limit and explicit acceptance checks. It accepts no credential, repository URL, arbitrary capability map, cloud provider or folder path.

The transaction reads the project's repository binding and writes it into the new card. The request UUID is the card UUID; an internal versioned receipt stores the original request alongside the frozen repository. An exact retry returns that original card, including after the project changes or disconnects its repository. Reusing the ID with a different task or target fails. Unknown/revoked target computers, missing repository bindings and invalid acceptance checks fail without creating a card.

Cards start **blocked / awaiting_repository_preparation**. They cannot be claimed, even by an otherwise eligible target worker. This API does not publish them to the ready queue. No new schema version/table is needed. Existing ordinary add_card and community submissions remain unchanged.

## Child tasks

LocalHub child creation now inherits repo_url and repo_ref from the parent card's frozen capabilities when the child has no explicit repository or workspace. It never reads the current project binding. The effective inherited values participate in existing same-key retry conflict checks. Repository children retain internet eligibility requirements. Explicit child locations win; imported parent folders are not inherited into another task.

This inherits repository/ref identity, not an exact starting commit. Shared-cache preparation resolves the ref later; exact-base pinning across a coordinated batch remains follow-up work. No credential is inherited.

## Verification

New regressions cover:
- Exact retry returns one identical frozen card after project disconnect; changed task/target request conflicts fail.
- Eligible workers cannot claim staged work before preparation.
- Unknown/revoked targets and invalid acceptance declarations produce no rows.
- Child retries retain the original repository after project changes; explicit folders win, imported parent folders are not shared.

Focused staging tests pass. Final workspace/lint outcomes are in continuity and /private/tmp/hive-private-stage-{tests,full,clippy}.log.

## Next implementation boundary

Expose staging through the existing primary-local, verified-owner FFI path only when it can be paired with authenticated preparation and visible task status/recovery. Preparation must use the frozen card repository, bind access to that task and target, validate the resulting durable workspace receipt, and only then activate the card. Retry must reconcile interrupted preparation rather than creating another task or deleting local work. Never activate merely because the user previously passed a repository-access preflight.

The desktop's current supervised worker still targets the community HubClient. A private LocalHub worker/session route is another required integration; do not expose a misleading Run action that queues jobs without a worker. This session did not change worker routing, clone private code, use live credentials, rebuild/deploy the app, push GitHub commits, or spend model tokens.

Sif your friendly Codex Agent
