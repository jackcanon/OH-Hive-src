# Community transcription: Honey pricing implementation

Implemented locally; not deployed or enabled. No numeric launch rate is seeded. Jack selected adding pricing before launching community transcription; the Honey-per-processing-minute amount is still awaiting his answer.

## Funding policy — Jack clarification, 2026-09-15

Transcription only processes funded work: fully funded upfront or funded as it goes. Interpret “fun as we go” as “fund as we go.” No unpaid processing or negative-balance credit is authorized.

Implementation requirement: reserve enough Honey for each admitted unit of work before dispatch, settle actual cost against that reservation, and release the unused balance. Concurrent jobs must not spend the same reserved Honey. For incremental funding, process independently funded chunks and wait for a top-up before the next unfunded chunk; do not rely on rejecting payment after the audio has already been processed. Chunking/checkpointing and reservation accounting are not implemented by the current one-pass speech path. Until they are, offer only fully funded jobs with an enforceable reservation, not an incremental-funding option.

Update 2026-09-16: source-aware reservations are implemented in `20260916050200_speech_reservations.sql` and tested locally. They protect the full approved cap at claim, exclude held funds from other ledger spending, settle atomically and release unused funds when the lease ends. The numeric tariff remains unresolved; chunked fund-as-you-go is still unimplemented.

## Contract for Claude and the submission UI

Apply `20260916050000_speech_rate_kind.sql`, `20260916050100_speech_honey_pricing.sql`, then `20260916050200_speech_reservations.sql` in order, committing each migration separately. Existing production migration-history reconciliation and replay gates still apply.

An authenticated administrator calls `public.hive_speech_rate_set(p_honey_per_minute)` with the approved positive rate. `public.hive_speech_rate()` returns `configured: false` until then; submission must remain unavailable. When configured it returns rate ID, Honey per second/minute and the 600-second ceiling. These are processing minutes, not source-audio minutes: slower workers can cost more for the same audio.

Create the speech card in a non-runnable state, display the rate and maximum charge, and obtain explicit approval. Call `public.hive_speech_price_card(p_card_id, p_rate_id, p_max_seconds, p_max_honey)` before making the card ready. Only its owning member may approve spending. A stale displayed rate fails for renewed review. The stored maximum is rounded to six decimals from approved seconds times the per-second rate. Identical retries return the existing receipt; changed inputs, rate or duration require a new card rather than mutating its approved price. Do not expose a ready card before pricing succeeds.

The lease insertion trigger requires the approved unchanged inputs and sufficient project balance for the maximum charge. Direct and delegated completion use the frozen rate and reported processing seconds, debit the project and credit the executing node member's wallet in the existing ledger transaction. Tokens must be zero; null, negative, nonfinite or over-budget duration fails. Expired leases and changed inputs fail. Settlement failures roll back output and completion rather than silently publishing unpaid work. Existing text pricing is preserved; local speech has no community Honey charge.

All Hive members can inspect the price receipts. This implements Jack's community openness policy; owner approval governs spending, not visibility. Existing authenticated node output attribution is retained.

## Validation

`scripts/test-speech-pricing.mjs` runs the migrations and actual direct/delegated settlement functions in PGlite with a focused schema fixture. It passes rate configuration/admin checks, nonmember and foreign-owner rejection, stale-rate review, explicit spending caps, frozen prices, both settlement paths, invalid usage, expired leases, changed inputs, insufficient-fund rollback, duplicate completion, lease admission, denied direct writes, preserved text billing and zero local speech billing. PGlite serializes database requests; this is not proof of production concurrency or a full historical migration replay.

`cargo clippy -p hive-core --all-features --lib --offline -- -D warnings` passes. The speech executor's deliberate argument count has a scoped allowance. Migration static checks and whitespace checks also pass.

## Remaining integration and limits

- No numeric rate, production migration, release build, deployment or paid submission is claimed. Wire submission/quote UI and RPCs, artifact association, capability advertisement and live acceptance next.
- Seconds are worker-reported, bounded by the requester-approved maximum; they are not independently metered. The ledger's duration field rounds to its existing millisecond precision while charge calculation uses the supplied numeric duration.
- Reservations now exclude held funds from both split debits and raw ledger transactions. Lease deletion releases holds on completion, cancellation and expiry reaping. Failed settlement preserves the hold and lease for retry. This does not guarantee a worker payout after its lease is revoked/expired, and it does not add independently metered time or chunk checkpoints. Existing unreserved speech leases must be released/requeued before rollout; completion will reject them.
- General claim selection does not skip invalid, unpriced ready speech cards. Such a card can make a claim fail; submission must price before ready, and existing invalid ready cards need to be blocked or repaired before rollout.
- The worker currently enforces the lease/600-second deadline, not a lower per-card approved duration. Until the worker receives that cap, submit only with the supported 600-second ceiling; smaller caps can cause work to be rejected at settlement.
- Comprehensive immutable receipts for every execution attempt, membership-removal acceptance and same-node lease-generation fencing remain separate work. No real audio/network/production accounting acceptance was performed here.

Sif your friendly Codex Agent
