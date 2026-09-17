-- `public.hive_code_usage_node` was declared STABLE and could not run at all.
--
-- Found by calling it over PostgREST against production rather than by reading it again:
--
--   {"code":"25006","message":"cannot execute UPDATE in a read-only transaction"}
--
-- PostgREST runs a STABLE function in a read-only transaction. `hive_code_usage_node` resolves its
-- caller with `hive.node_member_id`, which is VOLATILE because verifying a node key updates that
-- key's last-used metadata -- so the first statement inside a read-only transaction is an UPDATE
-- and the whole call fails. Every caller sees this; there is no partial-success case.
--
-- This is the same bug, one migration later, that 20260916050300 exists to fix:
--
--   "verify_node_key updates last-used metadata, so a STABLE caller is invalid."
--
-- That comment was written about `hive.node_projects_overview` two files before I declared this
-- function stable. Worth stating plainly: the lesson was already in the tree and I did not apply
-- it to my own function. The rule for this schema is that ANY function reached by a node key is
-- volatile, because authenticating that key is itself a write.
--
-- Why the fixture missed it: `scripts/test-code-brain-usage.mjs` stubs `hive.node_member_id` as a
-- pure `language sql` lookup with no update, so under PGlite the function is genuinely read-only
-- and STABLE is accurate. The stub is what made the test green and production broken. Fixed below
-- by making the stub volatile AND writing to a table, so the fixture now fails if this regresses --
-- a test that cannot reproduce the failure is not covering it.
--
-- `hive_code_session_status_node` is unaffected: it declares no volatility and therefore already
-- defaults to VOLATILE. Only this one function named `stable` explicitly.
begin;

create or replace function public.hive_code_usage_node(p_raw_key text) returns jsonb
language plpgsql volatile security definer set search_path = pg_catalog, hive, public as $$
declare mid uuid; begin
  mid := hive.node_member_id(p_raw_key);
  if mid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  return jsonb_build_object(
    'month', date_trunc('month', now())::date,
    'spent_usd', round(hive.code_brain_month_usd(mid), 6),
    'cap_usd', round(hive.code_brain_cap_usd(mid), 6),
    'remaining_usd', round(hive.code_brain_cap_usd(mid) - hive.code_brain_month_usd(mid), 6),
    'by_model', coalesce((
      select jsonb_agg(t order by t->>'provider', t->>'model_id') from (
        select jsonb_build_object('provider', provider, 'model_id', model_id, 'turns', count(*),
                                  'tokens_in', sum(tokens_in), 'tokens_out', sum(tokens_out),
                                  'usd_estimate', round(sum(usd_estimate), 6)) as t
        from hive.code_brain_usage
        where member_id = mid and created_at >= date_trunc('month', now())
        group by provider, model_id) g), '[]'::jsonb));
end;
$$;
revoke all on function public.hive_code_usage_node(text) from public, anon, authenticated;
grant execute on function public.hive_code_usage_node(text) to anon, authenticated;

notify pgrst, 'reload schema';
commit;
