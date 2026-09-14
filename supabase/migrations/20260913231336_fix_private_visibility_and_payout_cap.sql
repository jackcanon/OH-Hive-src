-- 2026-09-13: Sif's Personal Fleet review (docs/SIF-PERSONAL-FLEET-RECOMMENDATIONS-2026-09-13.md)
-- verified two live findings against this database. Jack has now made private-fleet control an
-- equal core pillar alongside community contribution, which makes both of these load-bearing
-- rather than cosmetic. Both confirmed live before this migration:
--
-- 1. Every SELECT policy for projects/cards/card_outputs/checkpoints/leases gated only on
--    hive.is_member() -- any active member could read any OTHER member's project, including
--    ones set execution_mode='local' (the private/personal-fleet mode added in #54). Several
--    SECURITY DEFINER RPCs (project_board, projects_overview, project_comments_list,
--    project_contributors) had the identical gap, independent of RLS.
-- 2. hive.node_complete_card computes the Honey payout from p_tokens_in/p_tokens_out supplied
--    directly by the completing node, with no server-side cap -- and posts the ledger debit
--    directly via post_txn rather than through split_debit, so the project fund can be driven
--    negative in one falsified completion, not just drained to zero.

-- ---------------------------------------------------------------------------
-- Part 1: private-project visibility
-- ---------------------------------------------------------------------------

create or replace function hive.project_visible(p_project_id uuid)
returns boolean
language sql stable security definer
set search_path to 'hive', 'public'
as $$
  select exists (
    select 1 from hive.projects p
    where p.id = p_project_id
      and (
        p.execution_mode = 'hive'
        or p.owner_id = auth.uid()
        or exists (select 1 from hive.project_roles r where r.project_id = p.id and r.member_id = auth.uid())
      )
  );
$$;

comment on function hive.project_visible(uuid) is
  'A hive-mode (community) project is visible to every member, matching the original design. A '
  'local-mode (private/personal-fleet) project is visible only to its owner and explicit '
  'project_roles collaborators. Used by RLS policies and by definer RPCs that were bypassing RLS '
  'entirely (project_board, projects_overview, project_comments_list, project_contributors) -- '
  'see the 2026-09-13 Sif review finding this migration fixes.';

create or replace function hive.card_visible(p_card_id uuid)
returns boolean
language sql stable security definer
set search_path to 'hive', 'public'
as $$
  select exists (select 1 from hive.cards c where c.id = p_card_id and hive.project_visible(c.project_id));
$$;

-- Base-table RLS: previously `using (hive.is_member())` on every one of these.
drop policy if exists projects_member_read on hive.projects;
create policy projects_member_read on hive.projects
  for select using (hive.is_member() and hive.project_visible(id));

drop policy if exists cards_member_read on hive.cards;
create policy cards_member_read on hive.cards
  for select using (hive.is_member() and hive.project_visible(project_id));

drop policy if exists card_outputs_member_read on hive.card_outputs;
create policy card_outputs_member_read on hive.card_outputs
  for select using (hive.is_member() and hive.card_visible(card_id));

drop policy if exists checkpoints_member_read on hive.checkpoints;
create policy checkpoints_member_read on hive.checkpoints
  for select using (hive.is_member() and hive.card_visible(card_id));

drop policy if exists leases_member_read on hive.leases;
create policy leases_member_read on hive.leases
  for select using (hive.is_member() and hive.card_visible(card_id));

-- artifacts.project_id is nullable (fleet-wide backups aren't tied to a project and stay
-- visible to every member, unchanged) -- only project-scoped artifacts get the new check.
drop policy if exists artifacts_member_read on hive.artifacts;
create policy artifacts_member_read on hive.artifacts
  for select using (hive.is_member() and (project_id is null or hive.project_visible(project_id)));

-- SECURITY DEFINER RPCs bypass RLS entirely -- each needs the same check inline, not just the
-- policy fix above.
create or replace function hive.project_board(p_project_id uuid)
 returns jsonb
 language sql
 stable security definer
 set search_path to 'hive', 'public'
as $function$
  select jsonb_build_object(
    'project', (select jsonb_build_object('id', p.id, 'title', p.title, 'goal', p.goal, 'license_kind', p.license_kind,
                  'license_spdx', p.license_spdx, 'requires_internet', p.requires_internet, 'plan', p.plan,
                  'owner', (select display_name from public.profiles where id = p.owner_id),
                  'my_role', hive.project_role(p.id), 'fund_balance', hive.account_balance(p.fund_account_id),
                  'execution_mode', p.execution_mode)
                from hive.projects p where p.id = p_project_id and p.deleted_at is null),
    'cards', coalesce((select jsonb_agg(jsonb_build_object(
        'id', c.id, 'key', c.key, 'title', c.title, 'modality', c.modality, 'status', c.status, 'inputs', c.inputs,
        'acceptance', c.acceptance, 'deps', c.deps, 'requires_internet', c.requires_internet,
        'required_capabilities', c.required_capabilities, 'order_index', c.order_index,
        'lease', (select jsonb_build_object('node', n.display_name, 'node_role', n.role, 'expires_at', l.expires_at)
                  from hive.leases l join hive.nodes n on n.id = l.node_id where l.card_id = c.id),
        'output', (select jsonb_build_object('content', o.content, 'model_id', o.model_id, 'usage', o.usage,
                          'node', n.display_name, 'node_role', n.role, 'created_at', o.created_at)
                   from hive.card_outputs o left join hive.nodes n on n.id = o.node_id where o.card_id = c.id order by o.created_at desc limit 1)
      ) order by c.order_index, c.created_at) from hive.cards c where c.project_id = p_project_id), '[]'::jsonb)
  ) where hive.is_member() and hive.project_visible(p_project_id);
$function$;

create or replace function hive.projects_overview()
 returns jsonb
 language sql
 stable security definer
 set search_path to 'hive', 'public'
as $function$
  select coalesce(jsonb_agg(jsonb_build_object(
    'id', p.id, 'title', p.title, 'goal', p.goal, 'license_kind', p.license_kind, 'license_spdx', p.license_spdx,
    'requires_internet', p.requires_internet, 'created_at', p.created_at,
    'owner', (select display_name from public.profiles where id = p.owner_id),
    'my_role', hive.project_role(p.id),
    'fund_balance', hive.account_balance(p.fund_account_id),
    'cards', (select jsonb_object_agg(s, n) from (select status::text s, count(*) n from hive.cards where project_id = p.id group by status) x)
  ) order by p.created_at desc), '[]'::jsonb)
  from hive.projects p where p.deleted_at is null and hive.is_member() and hive.project_visible(p.id);
$function$;

create or replace function hive.project_comments_list(p_project_id uuid)
 returns jsonb
 language sql
 stable security definer
 set search_path to 'hive', 'public'
as $function$
  select coalesce(jsonb_agg(jsonb_build_object(
    'id', c.id,
    'parent_comment_id', c.parent_comment_id,
    'author_id', c.author_id,
    'author', coalesce(pr.display_name, 'a member'),
    'body', case when c.deleted_at is null then c.body else null end,
    'deleted', c.deleted_at is not null,
    'created_at', c.created_at,
    'edited_at', c.edited_at,
    'is_mine', c.author_id = auth.uid()
  ) order by c.created_at), '[]'::jsonb)
  from hive.project_comments c
  left join public.profiles pr on pr.id = c.author_id
  where c.project_id = p_project_id and hive.is_member() and hive.project_visible(p_project_id)
    and exists (select 1 from hive.projects p where p.id = p_project_id and p.deleted_at is null);
$function$;

create or replace function hive.project_contributors(p_project_id uuid)
 returns jsonb
 language sql
 stable security definer
 set search_path to 'hive', 'public'
as $function$
  with attributed as (
    select fc.amount_honey, fc.anonymous, fc.created_at, a.member_id
    from hive.ledger_entries fc
    join hive.projects p on p.fund_account_id = fc.account_id
    join hive.ledger_entries d on d.txn_id = fc.txn_id and d.direction = 'debit' and d.entry_type = 'fund_project' and d.source = fc.source
    join hive.accounts a on a.id = d.account_id and a.kind = 'member_wallet'
    where p.id = p_project_id and fc.entry_type = 'fund_project' and fc.direction = 'credit'
  ),
  credited_grouped as (
    select member_id, sum(amount_honey) as total, max(created_at) as last_at
    from attributed where not anonymous group by member_id
  )
  select jsonb_build_object(
    'credited', coalesce((
      select jsonb_agg(jsonb_build_object(
          'member_id', cg.member_id, 'display_name', coalesce(pr.display_name, 'a member'),
          'total_honey', cg.total, 'last_at', cg.last_at) order by cg.total desc)
      from credited_grouped cg left join public.profiles pr on pr.id = cg.member_id
    ), '[]'::jsonb),
    'anonymous_total', coalesce((select sum(amount_honey) from attributed where anonymous), 0),
    'anonymous_count', coalesce((select count(*) from attributed where anonymous), 0)
  ) where hive.is_member() and hive.project_visible(p_project_id);
$function$;

create or replace function hive.project_comment_create(p_project_id uuid, p_body text, p_parent_comment_id uuid DEFAULT NULL::uuid)
 returns jsonb
 language plpgsql
 security definer
 set search_path to 'hive', 'public'
as $function$
declare cid uuid; body text; begin
  if not hive.is_member() then raise exception 'not_a_hive_member'; end if;
  body := trim(p_body);
  if body = '' then raise exception 'empty_comment'; end if;
  if length(body) > 8000 then raise exception 'comment_too_long'; end if;
  if not exists (select 1 from hive.projects where id = p_project_id and deleted_at is null) then
    raise exception 'project_not_found';
  end if;
  if not hive.project_visible(p_project_id) then raise exception 'project_not_found'; end if;
  if p_parent_comment_id is not null and not exists (
    select 1 from hive.project_comments where id = p_parent_comment_id and project_id = p_project_id
  ) then
    raise exception 'parent_comment_not_found';
  end if;
  insert into hive.project_comments (project_id, author_id, parent_comment_id, body)
  values (p_project_id, auth.uid(), p_parent_comment_id, body)
  returning id into cid;
  return jsonb_build_object('id', cid);
end $function$;

-- ---------------------------------------------------------------------------
-- Part 2: cap the Honey payout at the project fund's actual balance
-- ---------------------------------------------------------------------------

create or replace function hive.node_complete_card(raw_key text, p_card_id uuid, p_content text, p_model_id text, p_tokens_in bigint, p_tokens_out bigint, p_compute_seconds numeric)
 returns jsonb
 language plpgsql
 security definer
 set search_path to 'hive', 'public'
as $function$
declare nid uuid; l hive.leases; c hive.cards; fund uuid; wallet uuid; r_out hive.rate_table; r_in hive.rate_table;
        amt numeric; fund_balance numeric; tid uuid; p_owner uuid; p_title text; begin
  nid := hive.verify_node_key(raw_key);
  if nid is null then raise exception 'invalid_or_revoked_node_key'; end if;
  select * into l from hive.leases where card_id = p_card_id and node_id = nid;
  if not found then raise exception 'no_lease_for_this_node'; end if;
  select * into c from hive.cards where id = p_card_id;

  insert into hive.card_outputs (card_id, node_id, content, model_id, usage)
  values (p_card_id, nid, p_content, p_model_id,
          jsonb_build_object('tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds));

  select fund_account_id into fund from hive.projects where id = c.project_id;
  select a.id into wallet from hive.accounts a join hive.nodes n on n.member_id = a.member_id where n.id = nid and a.kind = 'member_wallet';
  r_out := hive.current_rate('compute_output'); r_in := hive.current_rate('compute_input');
  amt := round(coalesce(p_tokens_out,0) * r_out.honey_per_unit + coalesce(p_tokens_in,0) * coalesce(r_in.honey_per_unit,0), 6);

  -- 2026-09-13 fix (Sif review, verified live): p_tokens_in/p_tokens_out are self-reported by
  -- the completing node with no independent verification. This does not stop a compromised node
  -- from claiming an inflated count and draining a project's entire fund in one completion, but
  -- it does stop the two failure modes that made it unbounded: post_txn only checks that debits
  -- and credits balance against EACH OTHER, not against the account's actual balance, and this
  -- function never called split_debit (the function that would have capped it) -- so a fund
  -- could previously be driven negative, not just to zero. Real anti-fraud (per-card payout
  -- ceilings, provider-verified token counts) is follow-up work, not this stopgap.
  fund_balance := greatest(hive.account_balance(fund), 0);
  amt := least(amt, fund_balance);

  if amt > 0 then
    tid := hive.post_txn(jsonb_build_array(
      jsonb_build_object('account_id', fund,   'entry_type', 'spend_job',    'direction', 'debit',  'amount', amt, 'rate_id', r_out.id,
                         'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds, 'card_id', p_card_id, 'node_id', nid),
      jsonb_build_object('account_id', wallet, 'entry_type', 'earn_compute', 'direction', 'credit', 'amount', amt, 'rate_id', r_out.id,
                         'tokens_in', p_tokens_in, 'tokens_out', p_tokens_out, 'compute_seconds', p_compute_seconds, 'card_id', p_card_id, 'node_id', nid)
    ), 'card ' || c.key || ' on ' || (select display_name from hive.nodes where id = nid));
  end if;

  delete from hive.leases where card_id = p_card_id;
  update hive.cards set status = 'review' where id = p_card_id;

  select owner_id, title into p_owner, p_title from hive.projects where id = c.project_id;
  insert into hive.notification_events (event_type, project_id, card_id, member_id, payload)
  values ('card_completed', c.project_id, p_card_id, p_owner,
          jsonb_build_object('card_title', c.title, 'project_title', p_title, 'earned_honey', amt, 'node_region', (select region from hive.nodes where id = nid)));

  return jsonb_build_object('status', 'review', 'earned_honey', amt, 'txn_id', tid,
                            'fund_balance', hive.account_balance(fund), 'wallet_balance', hive.account_balance(wallet));
end $function$;
