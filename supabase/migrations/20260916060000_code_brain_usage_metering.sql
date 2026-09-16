-- Meter cloud code-card spend, and give it a ceiling (Cmd Work 90baee32).
--
-- What was actually wrong. `hive.provider_budget` looks like it caps cloud spend: there is a row
-- for this month with usd_cap 25. It does not cap this path. `hive.provider_budget_reserve` is
-- called by exactly two functions -- `hive.charge_interview` and `hive.charge_media` -- and the
-- `code-brain-turn` Edge Function calls neither. It authenticates the node key, fetches the
-- member's own provider key and preferred model, calls the provider, and returns. Nothing reads a
-- budget and nothing records what was spent. A cloud coding session is unmetered and uncapped;
-- the only bound is `p_max_turns` (default 40), and because each turn resends the whole transcript
-- the cost per turn RISES as the session runs.
--
-- Why this is deliberately NOT wired into `hive.provider_budget`. That table is Hive's own money --
-- dollars Anthropic bills US, for the interviewer and for image generation, which the Hive pays for
-- and recovers in $honey. A code card is always BYOK: `handler.ts` fetches
-- `hive_admin_member_key(member, provider)`, so the member's own API key makes the call and the
-- member's own card is charged by the provider directly. Debiting `provider_budget` for that would
-- book someone else's spend against Hive's ledger, and charging $honey for it on top (the shape
-- `charge_interview` uses) would bill the member twice for one turn. So this migration does not
-- touch the ledger, `provider_budget`, or $honey at all. It records what the member spent on their
-- own key and stops the session when their own monthly ceiling is reached. Different money,
-- different table.
--
-- Ordering, and the honest bound on it. A turn's cost is not knowable before the provider answers,
-- so this cannot reserve the real amount up front the way `provider_budget_reserve` does. Instead:
-- the Edge Function asks `hive_admin_code_brain_guard` BEFORE calling the provider (refuse if the
-- month is already at the ceiling) and `hive_admin_code_brain_record` AFTER (book the actual
-- tokens). The overshoot is therefore bounded by one turn, not by zero. One turn of a long
-- transcript is real money, so the guard reports `remaining_usd` and the caller is expected to
-- stop on a small remainder rather than treating "not yet over" as permission for another turn.
--
-- Not in this migration, and why: per-CARD attribution. The wire contract in
-- `supabase/functions/code-brain-turn/protocol.ts` is `{raw_key, provider, model, messages, tools}`
-- -- there is no card id, so the Edge Function literally cannot say which card a turn belongs to.
-- Adding one is a protocol field plus a line in `CloudBrain` (crates/), which is not this change.
-- Everything here keys on member + month, which is what a spending ceiling needs anyway.
begin;

-- ── One row per cloud brain turn ───────────────────────────────────────────────────────────────
-- `model_id` is recorded as the model actually used (post-resolution), not the request's
-- preference, because that is the one that priced the turn.
create table if not exists hive.code_brain_usage (
  id           uuid primary key default gen_random_uuid(),
  member_id    uuid not null references hive.members(id) on delete cascade,
  provider     text not null check (provider in ('anthropic', 'openai', 'nous')),
  model_id     text not null check (length(model_id) between 1 and 200),
  tokens_in    bigint not null check (tokens_in >= 0),
  tokens_out   bigint not null check (tokens_out >= 0),
  usd_estimate numeric not null check (usd_estimate >= 0),
  created_at   timestamptz not null default now()
);
create index if not exists code_brain_usage_member_time_idx on hive.code_brain_usage(member_id, created_at desc);
alter table hive.code_brain_usage enable row level security;
drop policy if exists code_brain_usage_own_read on hive.code_brain_usage;
-- Own rows only. A member's provider spend is not other members' business, so this is narrower
-- than the `hive.is_member()` read policy most tables here use.
create policy code_brain_usage_own_read on hive.code_brain_usage for select to authenticated using (member_id = auth.uid());
grant select on hive.code_brain_usage to authenticated;

-- ── The ceiling ────────────────────────────────────────────────────────────────────────────────
-- A per-member override; absent a row, the fleet default in `hive.settings`.
create table if not exists hive.code_brain_caps (
  member_id      uuid primary key references hive.members(id) on delete cascade,
  usd_cap_month  numeric not null check (usd_cap_month >= 0),
  updated_at     timestamptz not null default now()
);
alter table hive.code_brain_caps enable row level security;
drop policy if exists code_brain_caps_own_read on hive.code_brain_caps;
create policy code_brain_caps_own_read on hive.code_brain_caps for select to authenticated using (member_id = auth.uid());
grant select on hive.code_brain_caps to authenticated;

-- 25 is NOT a considered number. It is the value already sitting in `hive.provider_budget.usd_cap`,
-- reused so this ships with a finite ceiling instead of an infinite one while the real figure is a
-- decision for the person paying the bill. Change it with:
--   update hive.settings set value = to_jsonb(<dollars>::numeric), updated_at = now()
--     where key = 'code_brain_usd_cap_month';
insert into hive.settings (key, value) values ('code_brain_usd_cap_month', to_jsonb(25::numeric))
on conflict (key) do nothing;

create or replace function hive.code_brain_cap_usd(p_member uuid) returns numeric
language sql stable security definer set search_path = pg_catalog, hive, public as $$
  select coalesce(
    (select usd_cap_month from hive.code_brain_caps where member_id = p_member),
    (select (value #>> '{}')::numeric from hive.settings where key = 'code_brain_usd_cap_month'),
    0);
$$;

create or replace function hive.code_brain_month_usd(p_member uuid) returns numeric
language sql stable security definer set search_path = pg_catalog, hive, public as $$
  select coalesce(sum(usd_estimate), 0) from hive.code_brain_usage
  where member_id = p_member and created_at >= date_trunc('month', now());
$$;

-- Called before the provider call. Raises rather than returning a flag: a caller that ignores a
-- boolean spends money, and every other guard in this schema (`provider_budget_reserve`,
-- `code_requires_local_project`) raises for the same reason.
create or replace function hive.code_brain_guard(p_member uuid) returns jsonb
language plpgsql stable security definer set search_path = pg_catalog, hive, public as $$
declare cap numeric := hive.code_brain_cap_usd(p_member); spent numeric := hive.code_brain_month_usd(p_member);
begin
  if spent >= cap then
    raise exception 'code_brain_month_cap_reached: % of % USD used this month', round(spent, 4), round(cap, 2);
  end if;
  return jsonb_build_object('spent_usd', round(spent, 6), 'cap_usd', round(cap, 6),
                            'remaining_usd', round(cap - spent, 6));
end;
$$;

-- Called after the provider answers. Prices come from the caller for the same reason
-- `hive.charge_interview` takes them as parameters: the model in use and its price are the Edge
-- Function's knowledge, not the database's, and hard-coding a table of model prices here would go
-- stale silently. `usd_estimate` is an ESTIMATE and is named that way -- the authoritative number
-- is the provider's own invoice against the member's key.
create or replace function hive.code_brain_record(
  p_member uuid, p_provider text, p_model text, p_tokens_in bigint, p_tokens_out bigint,
  p_usd_in_per_m numeric, p_usd_out_per_m numeric
) returns jsonb
language plpgsql security definer set search_path = pg_catalog, hive, public as $$
declare usd numeric; spent numeric; cap numeric; begin
  if not exists (select 1 from hive.members where id = p_member and status = 'active') then
    raise exception 'not_a_hive_member';
  end if;
  usd := (coalesce(p_tokens_in, 0) * coalesce(p_usd_in_per_m, 0)
          + coalesce(p_tokens_out, 0) * coalesce(p_usd_out_per_m, 0)) / 1000000.0;
  insert into hive.code_brain_usage (member_id, provider, model_id, tokens_in, tokens_out, usd_estimate)
  values (p_member, p_provider, left(coalesce(nullif(btrim(p_model), ''), 'unknown'), 200),
          greatest(coalesce(p_tokens_in, 0), 0), greatest(coalesce(p_tokens_out, 0), 0), greatest(usd, 0));
  cap := hive.code_brain_cap_usd(p_member); spent := hive.code_brain_month_usd(p_member);
  return jsonb_build_object('turn_usd', round(usd, 6), 'spent_usd', round(spent, 6),
                            'cap_usd', round(cap, 6), 'remaining_usd', round(cap - spent, 6));
end;
$$;

-- ── Service-role front doors for the Edge Function ─────────────────────────────────────────────
-- Same shape and trust boundary as `hive_admin_code_brain_member` / `hive_admin_member_key`: these
-- take an arbitrary p_member rather than auth.uid(), so service_role only -- never anon or
-- authenticated. 20260907231452 exists because that grant was once forgotten; it is explicit here.
create or replace function public.hive_admin_code_brain_guard(p_member uuid) returns jsonb
language sql volatile security definer set search_path = pg_catalog, hive, public as $$
  select hive.code_brain_guard(p_member);
$$;
revoke all on function public.hive_admin_code_brain_guard(uuid) from public, anon, authenticated;
grant execute on function public.hive_admin_code_brain_guard(uuid) to service_role, postgres;

create or replace function public.hive_admin_code_brain_record(
  p_member uuid, p_provider text, p_model text, p_tokens_in bigint, p_tokens_out bigint,
  p_usd_in_per_m numeric, p_usd_out_per_m numeric
) returns jsonb
language sql volatile security definer set search_path = pg_catalog, hive, public as $$
  select hive.code_brain_record(p_member, p_provider, p_model, p_tokens_in, p_tokens_out,
                                p_usd_in_per_m, p_usd_out_per_m);
$$;
revoke all on function public.hive_admin_code_brain_record(uuid,text,text,bigint,bigint,numeric,numeric) from public, anon, authenticated;
grant execute on function public.hive_admin_code_brain_record(uuid,text,text,bigint,bigint,numeric,numeric) to service_role, postgres;

-- ── Node read-back: what did this month cost, and how much is left ────────────────────────────
-- The harness question that started this ("what did that run cost?") answered without a database
-- credential -- same node-key front door as `hive_code_session_status_node`.
create or replace function public.hive_code_usage_node(p_raw_key text) returns jsonb
language plpgsql stable security definer set search_path = pg_catalog, hive, public as $$
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

-- ── Status RPC: stop hiding the settlement numbers that were already there ─────────────────────
-- `hive.card_outputs` has carried `model_id` and `usage` since 20260905000004 and this function has
-- never returned either, which is why polling a finished card could not say what it cost. Changed
-- to one lateral join over the newest output instead of a scalar subquery on `content`, so all four
-- fields come from the SAME row -- a correlated subquery per field could straddle two outputs.
-- `latest_output` keeps its exact prior semantics, including null when a card has no output yet.
--
-- Read this together with the note at the top: for a code card these numbers are whatever the node
-- reported to `node_complete_card`, and today that is `Usage::default()` -- zeros -- because
-- `BrainTurn` (crates/ohhive-core/src/brain.rs:199) has no usage field to carry the counts
-- `normalize()` already puts on the wire. So expect zeros from a code card here until that lands,
-- and read `hive_code_usage_node` above for the number that is real now. Exposing a field that
-- currently reads zero is the lesser problem: interview and speech cards populate it correctly
-- already, and the field has to exist before anything can fill it.
create or replace function public.hive_code_session_status_node(p_raw_key text, p_card_id uuid) returns jsonb
language plpgsql security definer set search_path = pg_catalog, hive, public as $$
declare mid uuid; row jsonb; begin
  mid := hive.node_member_id(p_raw_key);
  if mid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select jsonb_build_object(
    'card_id', c.id, 'project_id', c.project_id, 'status', c.status, 'title', c.title,
    'key', c.key, 'created_at', c.created_at,
    'latest_output', o.content,
    'latest_output_at', o.created_at,
    'model_id', o.model_id,
    'usage', o.usage
  ) into row
  from hive.cards c
  join hive.projects p on p.id = c.project_id
  left join lateral (
    select o2.content, o2.created_at, o2.model_id, o2.usage
    from hive.card_outputs o2 where o2.card_id = c.id
    order by o2.created_at desc limit 1
  ) o on true
  where c.id = p_card_id and p.owner_id = mid;
  if row is null then raise exception 'card_not_found'; end if;
  return row;
end;
$$;
revoke all on function public.hive_code_session_status_node(text,uuid) from public, anon, authenticated;
grant execute on function public.hive_code_session_status_node(text,uuid) to anon, authenticated;

notify pgrst, 'reload schema';
commit;
