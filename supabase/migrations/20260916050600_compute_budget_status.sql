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
   'approved',b.card_id is not null,'valid',valid,'max_honey',b.max_honey,
   'spent',case when b.card_id is not null then b.max_honey-remaining else 0 end,
   'remaining',remaining,'reserved',coalesce(reserved,0),
   'funded',valid and remaining>0 and (reserved>=remaining or available>=remaining),
   'payer',case when b.interview_session_id is not null then 'member_wallet' else 'project_fund' end);
end $$;
revoke all on function public.hive_compute_budget_status(uuid) from public,anon;
grant execute on function public.hive_compute_budget_status(uuid) to authenticated;
