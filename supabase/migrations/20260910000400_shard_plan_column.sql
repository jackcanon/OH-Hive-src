-- Hive — shard_plan column for hive.cards (ADR-012 D16, Project Halo)
--
-- ADR-012 decision 16 has claimed since 2026-09-04 that hive.cards carries a
-- nullable shard_plan column "from day 1 so no migration is needed." That was
-- never actually true -- checked 2026-09-10, the column was never added in
-- any prior migration. This closes the gap between the ADR and the live
-- schema.
--
-- Still unused in v1: nothing writes or reads this column yet. Per Project
-- Halo lesson L6 (docs/HALO-V2-INTEGRATION-LESSONS.md), it should not become
-- load-bearing until R7 (shard-death mid-generation) has been run and the
-- checkpoint/resume story for a dropped shard-holder is designed from what
-- actually happens -- not before. Adding the column now only removes the
-- doc/schema mismatch; it does not turn on any v2 behavior.

alter table hive.cards
  add column if not exists shard_plan jsonb;

comment on column hive.cards.shard_plan is
  'Reserved for Project Halo (v2) pooled-inference placement -- which nodes '
  'hold which shard, at what tensor-split. Unused in v1. Do not populate for '
  'real jobs before R7 (shard-death mid-generation) has defined the '
  'checkpoint/resume behavior -- see docs/HALO-V2-INTEGRATION-LESSONS.md L6.';
