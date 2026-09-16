-- Reapproval is an explicit owner action, never an automatic retry allowance.
create table hive.compute_budget_history (
 id uuid primary key default gen_random_uuid(),
 card_id uuid not null references hive.cards(id),
 replaced_at timestamptz not null default clock_timestamp(),
 replaced_by uuid not null references auth.users(id),
 previous_approval jsonb not null
);
alter table hive.compute_budget_history enable row level security;
revoke all on hive.compute_budget_history from public,anon,authenticated;
grant select on hive.compute_budget_history to authenticated;
create policy compute_budget_history_member_read on hive.compute_budget_history for select to authenticated using(hive.is_member());

create or replace function public.hive_compute_budget_replace(
 p_card_id uuid,p_max_honey numeric,p_expected_approval timestamptz,p_expected_input_hash text
) returns jsonb language plpgsql security definer set search_path=hive,public as $$
declare c hive.cards; p hive.projects; b hive.compute_card_budgets; spent numeric; result jsonb;
begin
 if hive.is_member() is not true then raise exception 'not_a_member'; end if;
 -- Claim paths lock the card before lease insertion. Never replace an in-flight budget.
 select * into c from hive.cards where id=p_card_id for update;
 select * into p from hive.projects where id=c.project_id for share;
 if p.id is null or p.owner_id is distinct from auth.uid() or p.execution_mode<>'hive' or p.deleted_at is not null then raise exception 'not_owned_hive_card'; end if;
 select * into b from hive.compute_card_budgets where card_id=c.id for update;
 if b.card_id is null or b.interview_session_id is not null then raise exception 'replace_project_budget_only'; end if;
 if b.approved_at is distinct from p_expected_approval or md5(c.inputs||c.required_capabilities::text) is distinct from p_expected_input_hash then raise exception 'budget_changed_refresh_required'; end if;
 if c.status not in ('suggested','ready','blocked') or exists(select from hive.leases where card_id=c.id) or exists(select from hive.speech_reservations where card_id=c.id) then raise exception 'compute_card_already_started'; end if;
 select coalesce(sum(amount_honey),0) into spent from hive.ledger_entries where card_id=c.id and entry_type='spend_job' and direction='debit';
 if p_max_honey is null or p_max_honey<=spent or p_max_honey::text in ('NaN','Infinity','-Infinity') then raise exception 'limit_must_exceed_spent'; end if;
 insert into hive.compute_budget_history(card_id,replaced_by,previous_approval) values(c.id,auth.uid(),to_jsonb(b));
 delete from hive.compute_card_budgets where card_id=c.id;
 result:=hive.freeze_compute_budget(c.id,p.fund_account_id,p_max_honey);
 update hive.compute_card_budgets set approved_at=clock_timestamp() where card_id=c.id returning to_jsonb(compute_card_budgets) into result;
 return result;
end $$;
revoke all on function public.hive_compute_budget_replace(uuid,numeric,timestamptz,text) from public,anon;
grant execute on function public.hive_compute_budget_replace(uuid,numeric,timestamptz,text) to authenticated;

-- Read model for the project board. Community members may inspect; only owners approve.
create or replace function public.hive_compute_budget_status(p_card_id uuid) returns jsonb
language plpgsql security definer set search_path=hive,public as $$
declare c hive.cards; p hive.projects; b hive.compute_card_budgets;
        remaining numeric; reserved numeric; available numeric; valid boolean:=false;
begin
 if hive.is_member() is not true then raise exception 'not_a_member'; end if;
 select * into c from hive.cards where id=p_card_id;
 select * into p from hive.projects where id=c.project_id;
 if p.id is null or p.deleted_at is not null or p.execution_mode<>'hive' then raise exception 'community_card_not_found'; end if;
 select * into b from hive.compute_card_budgets where card_id=c.id;
 if b.card_id is not null then
   remaining:=hive.compute_budget_remaining(c.id);
   begin perform hive.validate_compute_budget(c.id); valid:=true; exception when others then valid:=false; end;
   select coalesce(sum(amount),0) into reserved from hive.speech_reservations where card_id=c.id;
   available:=greatest(hive.account_balance(b.payer_account_id)-coalesce((select sum(amount) from hive.speech_reservations where account_id=b.payer_account_id),0),0);
 end if;
 return jsonb_build_object(
   'can_approve',p.owner_id=auth.uid() and c.modality='text' and b.card_id is null and c.status in ('suggested','ready','blocked') and not exists(select from hive.leases where card_id=c.id),
   'can_replace',p.owner_id=auth.uid() and c.modality in ('text','code') and b.card_id is not null and b.interview_session_id is null and c.status in ('suggested','ready','blocked') and not exists(select from hive.leases where card_id=c.id) and not exists(select from hive.speech_reservations where card_id=c.id),
   'approval_version',b.approved_at,'input_hash',md5(c.inputs||c.required_capabilities::text),
   'approved',b.card_id is not null,'valid',valid,'max_honey',b.max_honey,
   'spent',case when b.card_id is not null then b.max_honey-remaining else 0 end,
   'remaining',remaining,'reserved',coalesce(reserved,0),
   'funded',valid and remaining>0 and (reserved>=remaining or available>=remaining),
   'payer',case when b.interview_session_id is not null then 'member_wallet' else 'project_fund' end);
end $$;
revoke all on function public.hive_compute_budget_status(uuid) from public,anon;
grant execute on function public.hive_compute_budget_status(uuid) to authenticated;
