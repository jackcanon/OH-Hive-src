-- Hive — interviewer support (ADR-006 D37–D39, ADR-002 §7 spend_interview).
-- The Edge Function runs as service_role and calls these. Safe to re-run.

-- Provider-API pricing rows (USD per token → honey via 1 honey = $0.01). Seed Sonnet-tier.
do $$ begin
  if not exists (select 1 from hive.rate_table where kind = 'api_provider_markup' and effective_to is null) then
    insert into hive.rate_table (kind, model_ref, honey_per_unit, note) values ('api_provider_markup', '*', 0, 'seed: resell at cost (ADR-002 open item)');
  end if;
end $$;

-- Charge a member for an interview turn at provider cost (through the peg) + markup. Balanced txn:
-- member wallet (debit) → provider_cost (credit). Returns remaining balance. Allows a small
-- negative balance so a member can finish the interview they started (they can't create a
-- funded project without topping up anyway).
create or replace function hive.charge_interview(p_member uuid, p_tokens_in bigint, p_tokens_out bigint,
                                                 p_usd_in_per_m numeric, p_usd_out_per_m numeric, p_memo text default 'interview turn')
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare wallet uuid; cost_usd numeric; amt numeric; markup numeric; tid uuid; begin
  select id into wallet from hive.accounts where kind = 'member_wallet' and member_id = p_member;
  if wallet is null then raise exception 'no_wallet_for_member'; end if;
  cost_usd := (coalesce(p_tokens_in,0) * p_usd_in_per_m + coalesce(p_tokens_out,0) * p_usd_out_per_m) / 1000000.0;
  select honey_per_unit into markup from hive.rate_table where kind = 'api_provider_markup' and effective_to is null order by effective_from desc limit 1;
  amt := round(cost_usd * 100 * (1 + coalesce(markup, 0)), 6);        -- 1 honey = $0.01
  if amt > 0 then
    tid := hive.post_txn(jsonb_build_array(
      jsonb_build_object('account_id', wallet, 'entry_type', 'spend_interview', 'direction', 'debit', 'amount', amt,
                         'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'memo', p_memo),
      jsonb_build_object('account_id', (select id from hive.accounts where kind = 'provider_cost'), 'entry_type', 'spend_interview',
                         'direction', 'credit', 'amount', amt, 'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'memo', p_memo)
    ));
  end if;
  return jsonb_build_object('charged', amt, 'txn_id', tid, 'balance', hive.account_balance(wallet));
end $$;
revoke all on function hive.charge_interview(uuid, bigint, bigint, numeric, numeric, text) from public;
grant execute on function hive.charge_interview(uuid, bigint, bigint, numeric, numeric, text) to service_role;

-- Materialize a validated ProjectPlan into projects + cards (owner = p_member). Cards start 'ready'.
create or replace function hive.create_project_from_plan(p_member uuid, p_plan jsonb)
returns jsonb language plpgsql security definer set search_path = hive, public as $$
declare pid uuid; c jsonb; i int := 0; begin
  if not exists (select 1 from hive.members where id = p_member and status = 'active') then raise exception 'not_a_hive_member'; end if;
  if (p_plan->>'schema_version')::int <> 1 then raise exception 'unsupported_plan_schema'; end if;
  insert into hive.projects (owner_id, title, goal, license_kind, license_spdx, requires_internet, plan)
  values (p_member, p_plan->>'title', p_plan->>'goal', (p_plan->'license'->>'kind')::hive.license_kind,
          p_plan->'license'->>'spdx', coalesce((p_plan->>'requires_internet')::boolean, false), p_plan)
  returning id into pid;
  for c in select * from jsonb_array_elements(p_plan->'cards') loop
    i := i + 1;
    insert into hive.cards (project_id, key, title, modality, inputs, acceptance, deps, requires_internet, required_capabilities, order_index)
    values (pid, c->>'key', c->>'title', (c->>'modality')::hive.modality, coalesce(c->>'inputs',''), coalesce(c->>'acceptance',''),
            coalesce((select array_agg(x) from jsonb_array_elements_text(coalesce(c->'deps','[]'::jsonb)) x), '{}'),
            coalesce((c->>'requires_internet')::boolean, false), coalesce(c->'required_capabilities', '{}'::jsonb), i);
  end loop;
  return jsonb_build_object('project_id', pid, 'cards', i);
end $$;
revoke all on function hive.create_project_from_plan(uuid, jsonb) from public;
grant execute on function hive.create_project_from_plan(uuid, jsonb) to service_role;

-- Node inventory the interviewer can mention ("the Hive currently has N text nodes, 0 video nodes").
create or replace function hive.capacity_summary() returns jsonb
language sql stable security definer set search_path = hive, public as $$
  select jsonb_build_object(
    'nodes_checked_in', (select count(*) from hive.nodes where presence = 'checked_in'),
    'modalities', (select coalesce(jsonb_object_agg(m, n), '{}'::jsonb) from (
        select m, count(*) n from hive.nodes, jsonb_array_elements_text(coalesce(capabilities->'modalities','[]'::jsonb)) m
        where presence = 'checked_in' group by m) x),
    'internet_nodes', (select count(*) from hive.nodes where presence = 'checked_in' and allow_internet),
    'models', (select coalesce(jsonb_agg(distinct m->>'id'), '[]'::jsonb) from hive.nodes, jsonb_array_elements(coalesce(capabilities->'models','[]'::jsonb)) m where presence = 'checked_in')
  );
$$;
grant execute on function hive.capacity_summary() to authenticated, service_role;
create or replace function public.hive_capacity_summary() returns jsonb language sql stable security definer set search_path = hive, public as $$ select hive.capacity_summary(); $$;
grant execute on function public.hive_capacity_summary() to authenticated, service_role;

-- Service-role wrappers in public (until schema hive is exposed to PostgREST).
create or replace function public.hive_admin_member_active(p_member uuid) returns boolean
language sql stable security definer set search_path = hive, public as $$
  select exists (select 1 from hive.members where id = p_member and status = 'active'); $$;
create or replace function public.hive_admin_charge_interview(p_member uuid, p_tokens_in bigint, p_tokens_out bigint, p_usd_in_per_m numeric, p_usd_out_per_m numeric, p_memo text default 'interview turn') returns jsonb
language sql security definer set search_path = hive, public as $$
  select hive.charge_interview(p_member, p_tokens_in, p_tokens_out, p_usd_in_per_m, p_usd_out_per_m, p_memo); $$;
create or replace function public.hive_admin_create_project_from_plan(p_member uuid, p_plan jsonb) returns jsonb
language sql security definer set search_path = hive, public as $$
  select hive.create_project_from_plan(p_member, p_plan); $$;
revoke all on function public.hive_admin_member_active(uuid) from public, anon, authenticated;
revoke all on function public.hive_admin_charge_interview(uuid, bigint, bigint, numeric, numeric, text) from public, anon, authenticated;
revoke all on function public.hive_admin_create_project_from_plan(uuid, jsonb) from public, anon, authenticated;
grant execute on function public.hive_admin_member_active(uuid) to service_role;
grant execute on function public.hive_admin_charge_interview(uuid, bigint, bigint, numeric, numeric, text) to service_role;
grant execute on function public.hive_admin_create_project_from_plan(uuid, jsonb) to service_role;
