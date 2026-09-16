-- Storage is explicitly sponsored from a funded project, never minted from treasury.
create table hive.storage_allowances (
 hash text not null,
 node_id uuid not null,
 approved_by uuid not null references public.profiles(id),
 payer_account_id uuid not null references hive.accounts(id),
 approved_bytes bigint not null check(approved_bytes>0),
 max_honey_per_day numeric not null check(max_honey_per_day>0),
 approved_at timestamptz not null default now(),
 primary key(hash,node_id),
 foreign key(hash,node_id) references hive.artifact_replicas(hash,node_id) on delete cascade
);
alter table hive.storage_allowances enable row level security;
revoke all on hive.storage_allowances from public,anon,authenticated;
grant select on hive.storage_allowances to authenticated;
create policy storage_allowance_member_read on hive.storage_allowances for select to authenticated using(hive.is_member());
create or replace function public.hive_storage_allowance_approve(p_hash text,p_node uuid,p_payer_project uuid,p_max_honey_per_day numeric) returns jsonb
language plpgsql security definer set search_path=hive,public as $$
declare a hive.artifacts; r hive.artifact_replicas; p hive.projects; b hive.storage_allowances;
begin
 if hive.is_member() is not true then raise exception 'not_a_member'; end if;
 if p_max_honey_per_day is null or p_max_honey_per_day<=0 or p_max_honey_per_day::text in ('NaN','Infinity','-Infinity') or round(p_max_honey_per_day,6)<>p_max_honey_per_day then raise exception 'invalid_storage_allowance'; end if;
 select * into r from hive.artifact_replicas where hash=p_hash and node_id=p_node for update;
 select * into a from hive.artifacts where hash=p_hash;
 select * into p from hive.projects where id=p_payer_project;
 if p.id is null or p.deleted_at is not null or p.execution_mode<>'hive' or
    (a.project_id is not null and (a.project_id<>p.id or p.owner_id is distinct from auth.uid())) or
    (a.project_id is null and hive.is_admin() is not true) then raise exception 'not_authorized_storage_sponsor'; end if;
 if r.hash is null or r.bytes<=0 or r.bytes is distinct from a.bytes then raise exception 'storage_size_mismatch'; end if;
 insert into hive.storage_allowances(hash,node_id,approved_by,payer_account_id,approved_bytes,max_honey_per_day)
 values(p_hash,p_node,auth.uid(),p.fund_account_id,r.bytes,p_max_honey_per_day)
 on conflict(hash,node_id) do update set approved_by=excluded.approved_by,payer_account_id=excluded.payer_account_id,approved_bytes=excluded.approved_bytes,max_honey_per_day=excluded.max_honey_per_day,approved_at=now()
 returning * into b;
 return to_jsonb(b);
end $$;
revoke all on function public.hive_storage_allowance_approve(text,uuid,uuid,numeric) from public,anon;
grant execute on function public.hive_storage_allowance_approve(text,uuid,uuid,numeric) to authenticated;
create or replace function hive.settle_storage() returns jsonb
language plpgsql security definer set search_path = hive, public as $$
declare t0 timestamptz := clock_timestamp(); r record; rate numeric; rate_id uuid; hours numeric; gb numeric; amt numeric;
        pool uuid; treasury uuid; fund uuid; wallet uuid; debits jsonb; n_rep int := 0; n_unpaid int := 0; tot_gbh numeric := 0; tot_charged numeric := 0; tot_paid numeric := 0;
begin
  select honey_per_unit, id into rate, rate_id from hive.rate_table where kind = 'storage_gb_hour' and effective_to is null order by effective_from desc limit 1;
  if rate is null or rate<=0 or rate::text in ('NaN','Infinity','-Infinity') then return jsonb_build_object('skipped', 'no storage rate'); end if;
  select id into pool from hive.accounts where kind = 'storage_pool';
  select id into treasury from hive.accounts where kind = 'treasury';

  for r in
    select rp.hash, rp.node_id, rp.bytes, greatest(coalesce(rp.last_settled_at, rp.announced_at), b.approved_at, now()-interval '24 hours') as since, b.max_honey_per_day, b.payer_account_id, a.project_id, a.pinned, a.kind,
           n.member_id as server_member
    from hive.artifact_replicas rp
    join hive.artifacts a on a.hash = rp.hash
    join hive.storage_allowances b on b.hash=rp.hash and b.node_id=rp.node_id and b.approved_bytes=rp.bytes and b.approved_bytes=a.bytes
    join hive.projects payer on payer.fund_account_id=b.payer_account_id and payer.deleted_at is null and payer.execution_mode='hive'
    join hive.members approver on approver.id=b.approved_by and approver.status='active'
    join hive.members recipient on recipient.id=(select member_id from hive.nodes where id=rp.node_id) and recipient.status='active' 
    join hive.regional_servers s on s.node_id = rp.node_id
    join hive.nodes n on n.id = rp.node_id
    where (a.project_id=payer.id and payer.owner_id=b.approved_by or a.project_id is null and approver.is_admin) and s.status = 'online' and coalesce(rp.last_settled_at, rp.announced_at) < now() - interval '1 hour'
    for update of rp skip locked
  loop
    hours := extract(epoch from now() - r.since) / 3600.0;
    gb := r.bytes / 1073741824.0;
    amt := trunc(least(gb * hours * rate, r.max_honey_per_day * hours / 24), 6);
    n_rep := n_rep + 1; tot_gbh := tot_gbh + gb * hours;
    if amt <= 0 then
      update hive.artifact_replicas set last_settled_at = now() where hash = r.hash and node_id = r.node_id;
      continue;
    end if;
    select id into wallet from hive.accounts where kind='member_wallet' and member_id=r.server_member;
    if wallet is null then continue; end if;
    fund:=r.payer_account_id;
    begin
      debits:=hive.split_debit(fund,amt,array['earned','grant','purchased'],jsonb_build_object('entry_type','storage_charge'));
    exception when others then
      update hive.artifacts set unpaid_since=coalesce(unpaid_since,now()) where hash=r.hash;
      n_unpaid:=n_unpaid+1;
      continue;
    end;
    perform hive.post_txn(debits || jsonb_build_array(jsonb_build_object('account_id', pool, 'entry_type', 'storage_charge', 'direction', 'credit', 'amount', amt, 'source', 'grant')),
                          'storage ' || left(r.hash, 8) || ' on ' || (select display_name from hive.nodes where id = r.node_id));
    tot_charged := tot_charged + amt;
    -- pay the server's owner
    select id into wallet from hive.accounts where kind = 'member_wallet' and member_id = r.server_member;
    if wallet is not null then
      perform hive.post_txn(jsonb_build_array(
        jsonb_build_object('account_id', pool,   'entry_type', 'earn_infra', 'direction', 'debit',  'amount', amt, 'source', 'grant', 'node_id', r.node_id, 'rate_id', rate_id),
        jsonb_build_object('account_id', wallet, 'entry_type', 'earn_infra', 'direction', 'credit', 'amount', amt, 'source', 'earned', 'node_id', r.node_id, 'rate_id', rate_id)
      ), 'storage ' || left(r.hash, 8) || ' held ' || round(hours, 1) || 'h');
      tot_paid := tot_paid + amt;
    end if;
    update hive.artifact_replicas set last_settled_at = now() where hash = r.hash and node_id = r.node_id;
    update hive.artifacts set last_settled_at = now(), unpaid_since = null where hash = r.hash;
  end loop;

  insert into hive.settlement_log (replicas, gb_hours, charged_honey, paid_honey, unpaid, duration_ms)
  values (n_rep, round(tot_gbh, 6), tot_charged, tot_paid, n_unpaid, (extract(epoch from clock_timestamp() - t0) * 1000)::int);
  delete from hive.settlement_log where ran_at < now() - interval '180 days';
  return jsonb_build_object('replicas', n_rep, 'gb_hours', round(tot_gbh, 6), 'charged', tot_charged, 'paid', tot_paid, 'unpaid', n_unpaid);
end $$;

