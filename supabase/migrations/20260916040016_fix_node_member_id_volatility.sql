-- APPLIED to production 2026-09-16 as version 20260916040016. This filename deliberately carries
-- the version Supabase recorded, rather than a tidier one -- see the drift note at the bottom.
--
-- Audit 4.7: hive_code_session_projects_node was declared STABLE while its call chain writes.
--
--   public.hive_code_session_projects_node  (STABLE)
--     -> hive.node_member_id                (STABLE)
--       -> hive.verify_node_key             (VOLATILE, UPDATEs hive.node_keys)
--
-- PostgreSQL refuses a data-modifying statement inside a non-volatile function, so every call
-- errored at runtime: `hive card submit --project <title>` was broken in production from the
-- moment 20260915144511_hive_card_submit_node_rpcs was applied. The audit predicted this would
-- break "once applied" and the repo copy of that file still says NOT YET APPLIED -- it was.
--
-- Exactly the failure 20260907181751_fix_node_summary_volatility already fixed for
-- hive.node_summary, which is correctly VOLATILE today. Same remedy, same reasoning.
--
-- ALTER FUNCTION rather than CREATE OR REPLACE: this changes only the volatility marker and
-- leaves each body, signature, owner and ACL untouched. Reverse by setting STABLE.
--
-- Verified before applying: these two were the only STABLE or IMMUTABLE functions in hive or
-- public whose bodies reference node_member_id, so no other caller was left broken.

ALTER FUNCTION hive.node_member_id(p_raw_key text) VOLATILE;
ALTER FUNCTION public.hive_code_session_projects_node(p_raw_key text) VOLATILE;

-- Drift note, recorded here because it is the live deployment hazard (audit 4.6):
-- twelve repo migrations carry different version numbers than production, because they were
-- applied with Supabase-assigned timestamps and later renumbered in the repo to a tidy sequence
-- (e.g. repo 20260913010000_chat_memory is production 20260913182139). To `supabase db push`
-- those twelve look UNAPPLIED, and several later files are not idempotent -- so a push today
-- would try to re-apply them. Reconcile with `supabase migration repair` before deploying the
-- six pending migrations. This file is named to match production so the gap does not widen.
