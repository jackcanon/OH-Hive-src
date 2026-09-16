# S-4 Honey safeguards — implementation and integration handoff

Implemented locally, not deployed. This closes the identified automatic treasury issuance and unbounded completion-payment paths with explicit approvals and funded claims. It is not a claim that the full rollout or all client controls are finished.

## General community compute

Migration `20260916050400_funded_compute_and_interviews.sql` introduces owner-approved card budgets. Call `hive_compute_budget_approve(p_card_id, p_max_honey)` as the owning active member before dispatch. The approval freezes input/output rates and a fingerprint of the prompt and required capabilities. The default selected for this implementation is an explicit per-card ceiling; no project-wide automatic allowance was invented.

The lease trigger reserves the remaining approved budget from the payer, using existing source-aware reservation infrastructure. `hive.speech_reservations` retains its historical name but now covers general compute too; all existing debit paths already exclude its holds. Direct and delegated completion reject invalid usage, expired leases, changed inputs/payers and over-budget totals. Ledger failure aborts completion rather than returning an unpaid success. Retry/send-back charges consume the same cumulative card allowance, not a fresh allowance. Lease deletion releases holds. The direct and both pilot claim functions skip unapproved/unfunded cards, including a high-priority unapproved queue head.

Text and code have token prices, but the existing restriction keeping code jobs private remains intact. Other community modalities without explicit pricing stay blocked; this does not fabricate tariffs for image/music/video. Private execution remains zero-Honey. This is a spending ceiling, not independent proof of a worker's reported token usage.

## Local community interviews

`hive_interview_send_funded(p_session, p_text, p_mode, p_max_honey)` binds each turn to its requesting member's wallet. It checks available funds at submission and reserves at claim. The shared Interviews project no longer tops itself up from the treasury. Completion pays directly from the member wallet; polling only gathers the result and displays the cost. Repeated polling cannot debit again. Legacy send calls fail with an explicit budget-required error.

The web Chat page now asks for a maximum Honey amount per reply and calls the funded endpoint. BYOK remains unchanged. Other callers of the legacy interview endpoint must migrate. No arbitrary spending default is seeded.

## Storage

Migration `20260916050500_approved_storage_funding.sql` introduces explicit per-replica approvals via `hive_storage_allowance_approve(p_hash, p_node, p_payer_project, p_max_honey_per_day)`.

Project owners sponsor their project's artifacts; admins can sponsor projectless infrastructure from an explicitly chosen funded Hive project. Settlement never draws directly from treasury. An allowance binds canonical artifact size and replica size; changed sizes require approval again. Settlement uses the existing storage rate, bounded by the approved daily ceiling prorated by elapsed hours, with at most 24 hours of backbilling and no billing before approval. Replica row locks prevent overlapping settlement from paying twice. Missing approval, inactive approver/recipient, changed ownership, or missing funds prevents payment. The pool charge and recipient payment occur in one database transaction.

Storage settlement remains payment for reported storage, not a cryptographic proof of possession. Approval bounds exposure; funding can run out, in which case existing unpaid handling applies. This is not a new prepaid storage service or automatic deletion policy.

## Verification

- Fresh PostgreSQL-compatible PGlite replay: all 95 migrations; RLS/publication guards and recovered RPC tests pass.
- New full-chain `scripts/migration-replay/funded-compute.mjs`: unauthorized approval, missing budget, protected reservations, over-cap rollback, successful completion, cumulative retry hold/release, actual interview dispatch despite unfunded shared project and unapproved queue head, settlement before poll, repeated poll neutrality, unapproved storage skip, prorated storage cap, duplicate settlement skip, changed-size rejection, no treasury draw, ledger balance.
- Existing speech pricing/reservation regression suite passes (its migration-stage fixture is separate from the full-chain test).
- Web TypeScript check, migration checks, and whitespace checks pass.
- Schema baseline: all 1,070 catalog objects and 326 baseline functions accounted for; 22 intentional pending body changes are documented in the manifest. New definitions are additive.
- Real multi-session PostgreSQL races, browser interaction and live workers have not been exercised by these tests.

## Before production

Claude: review these migrations together with the earlier pending Honey migrations and migration-history reconciliation. Do not apply the reference schema snapshot over them.

1. Web project-board text-card approval controls are now built (follow-up below). Other project clients and storage-allowance controls for owners/admins still need integration. Existing queued jobs and replicas intentionally do not receive implicit approvals. Existing in-flight leases need a drain/requeue migration plan before enforcement.
2. Propagate approved remaining budgets into worker execution limits. The server prevents excess payment today; a worker can still waste effort and have an over-cap completion rejected. Existing speech duration-cap propagation is also still pending. Incremental/fund-as-you-go chunks remain separate work.
3. Provide an explicit reapproval/replacement workflow for changed or exhausted card budgets; current approvals are immutable. A new card can obtain a new approval. Add storage allowance revocation UI/API before broad operator rollout.
4. Run genuine PostgreSQL contention tests and direct/delegated live acceptance after reconciling migration history. Configure the still-unset speech tariff separately.

No commit, push, deployment, production billing change or remote CI result is claimed. After S-4 integration review, queue S-5 is held-chain Release controls inside agent activity logs.

Sif your friendly Codex Agent


## Follow-up: web project-board approvals

Migration `20260916050600_compute_budget_status.sql` adds a member-only community-card read RPC. It returns owner-only approval eligibility, total approved/spent/remaining/reserved Honey, and whether the applicable payer can fund the remaining allowance. Private and deleted projects are rejected. It does not return the payer's wallet balance. Existing mutation RPC remains the authority for all approvals.

`apps/web/components/ComputeBudget.tsx` is mounted only for an expanded community text card in `apps/web/app/projects/[id]/page.tsx`. Owners can enter and approve an explicit ceiling. Members see the budget and funding state, with manual refresh and refresh after status changes/approval. Limits are not editable after approval; replacement approvals remain separate work. Funding eligibility is a snapshot, with the existing lease trigger rechecking at actual admission. No new polling loop or per-collapsed-card requests added.

Verification: all 96 migrations replay; tests cover owner/non-owner approval eligibility, approved state, funded state, settled totals and denial of private-card reads. Web TypeScript, schema baseline (unchanged 22 intentional modifications), migration checks, and whitespace checks pass. React review covered effect cleanup, stale-response suppression, accessible labeled numeric input and status/errors, explicit approval action, and server authorization. No browser interaction or deployment claimed.

## Follow-up: project storage allowance controls

Migration `20260916050700_storage_allowance_controls.sql` adds member-visible, paged (100 rows) community-project replica/allowance listing and owner-authorized revocation. Private and deleted projects are rejected by listing. Revocation locks the replica consistently with approval/settlement, is idempotent, and does not refund completed payments or delete files. Infrastructure/projectless revocation requires Hive admin, but infrastructure sponsorship UI is not included in this slice.

Settlement re-reads the allowance after acquiring the replica lock and skips a stale cursor record if approval identity/time/size/cap/payer changed. This covers a revoke/update between cursor snapshot and lock acquisition at the usual READ COMMITTED isolation; genuine concurrent PostgreSQL acceptance remains outstanding.

`apps/web/components/StorageAllowances.tsx` adds an expandable Storage payments section to community project boards. Owners can approve/change a per-copy daily limit or stop an allowance; all members can inspect. Shows stored-size mismatches, unavailable server status, and past insufficient funding. No requests until expanded, stale response cleanup, manual pagination/refresh, explicit mutation buttons and server-side authorization. Does not automatically fund projects, delete data or enable treasury issuance.

Verification: all 97 migrations replay; full-chain tests now cover owner/non-owner listing, private-project denial, unauthorized revoke denial, repeat revoke and no settlement after revocation even when size is restored. Web TypeScript, migration guard, whitespace and baseline comparison pass (same 22 intentional body changes). No browser or multi-session live test, deployment, commit or push claimed. Remaining S-4 work includes infrastructure admin UI, replacement compute approvals, worker-side caps and rollout acceptance.
